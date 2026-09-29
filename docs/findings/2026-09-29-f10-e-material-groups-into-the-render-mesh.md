# F10-E: every stored material group reaches the render mesh

**Task:** #382 (`F10-E`), closing fidelity gap **F10-C.03**, which #366 measured
and deliberately did not fix because the fix was not its slice.
**Shared contract:** `docs/contracts/IDENTITY-CONTENT.md`.
**Test prefix:** `accept_f10_e_`.
**Capabilities used:** ordinary build/test, and `retail` for the one
`#[ignore = "requires CS_GAME_DIR"]` test. Implementer: bunny-2.

## The gap, in one sentence

The CS GameZ layout stores **one material group per polygon**, each with its own
raw material index and its own per-corner UV set; F10-A's `RawMesh` mirrors only
the **first** group onto its single-valued `material` and `uv`; so a render mesh
built from that IR gave every later group no UV set and no material at all.

## 1. The measurement, per archive

Measured on the original installation with the production reader
(`read_gamez_meshes`) over all nine GameZ archives, before any code changed.
Every figure below is a count of **stored polygons** with `mat_count > 1`, not of
groups, and not of meshes.

| Archive | present meshes | polygons | groups per polygon | multi-group polygons | meshes holding one | extra draws if every group is drawn |
| --- | --- | --- | --- | --- | --- | --- |
| `ZBD/planes.zbd` (airframes) | 1 766 | 16 200 | `{1: 16200}` | **0** | 0 | 0 |
| `ZBD/C1/gamez.zbd` | 2 237 | 18 277 | `{1: 17887, 2: 385, 3: 5}` | 390 | 61 | 1 042 |
| `ZBD/C1B/gamez.zbd` | 1 305 | 8 323 | `{1: 8315, 2: 8}` | 8 | 8 | 16 |
| `ZBD/C1C/gamez.zbd` | 1 518 | 8 040 | `{1: 8030, 2: 10}` | 10 | 10 | 20 |
| `ZBD/C2/gamez.zbd` | 1 765 | 12 645 | `{1: 12439, 2: 204, 3: 2}` | 206 | 73 | 552 |
| `ZBD/C2B/gamez.zbd` | 1 365 | 7 008 | `{1: 7000, 2: 8}` | 8 | 8 | 16 |
| `ZBD/C3/gamez.zbd` | 1 901 | 16 087 | `{1: 16071, 2: 16}` | 16 | 12 | 32 |
| `ZBD/C4/gamez.zbd` | 2 431 | 19 661 | `{1: 19645, 2: 16}` | 16 | 12 | 32 |
| `ZBD/C5/gamez.zbd` | 2 851 | 22 493 | `{1: 22141, 2: 352}` | 352 | 123 | 785 |
| **total** | **17 139** | **128 734** | `{1: 127728, 2: 999, 3: 7}` | **1 006** | **307** | **2 495** |

### Which worlds, and which airframes

* **World geometry: all eight world archives.** C1 390, C5 352, C2 206, C3 16,
  C4 16, C1C 10, C1B 8, C2B 8.
* **Airframes: none.** `ZBD/planes.zbd` stores **zero** multi-group polygons
  across all 16 200 of its polygons in 1 766 stored meshes. This confirms on the
  other half of the corpus what F10-B measured and what F10-C recorded, so the
  claim "the reason can only ever fire for world geometry" is now a per-archive
  measurement rather than an inference from one container.

### Four further facts the fix had to know

| Measured | Value | Why it matters |
| --- | --- | --- |
| Multi-group polygons with **at least two distinct raw material indices** | **1 006 of 1 006** | A dropped group is never a redundant copy: every one of them merges two *different* authored materials. The measured 17 polygons in C1, C3 and C4 whose groups store *identical* UV sets still differ in material, so the vertex key's material half is load-bearing on real data and not only on the fixture. |
| Multi-group polygons that are **outlines** vs **strips** | 646 outlines, 360 strips (C1: 117/273, C2: 134/72, C5: 338/14) | Both topologies have to carry the groups; neither is a corner case. |
| Corner counts of multi-group polygons | 3, 4, 5, 6, 7, 8, 10, 11, 12, 14, 15, 16, 18, 33 | A 33-corner strip with 2 or 3 groups is real; the fix is not a triangle-only path. |
| Seams that exist **only** in a group beyond the first | C1 1, C2 10, C5 11 (**22** in all) | The direct measure of what the first-group mirroring could not show. AC03 for the second group is not hypothetical: 22 positions in the installation are split by a coordinate set that no earlier stage could reach. |

**The count is not zero anywhere**, so the claim under test is a real fidelity
statement about 307 of 17 139 stored meshes, not a corner.

## 2. The IR decision

`RawMesh` is stage F10-A's published contract and several crates build against
it. The task offered three options "in order of preference" and asked for the
decision to be taken with the owner. Recorded here as the decision and its
reasoning, so a reviewer can check it and the owner can overrule it:

* **Option (a), a lossless field beside the polygons** — **already done** by
  #363: `GameZMesh::material_groups` holds one entry per stored polygon, and
  `GameZMesh::groups` / `corner_uv` reach them. It was the right shape and it is
  kept. What was missing was a *consumer*: nothing built a render mesh out of it.
* **Option (b), a new `RenderMesh` input that carries all groups, leaving
  `RawMesh` alone** — **chosen.** `RenderMesh::from_stored_groups` takes the
  stored group table and is what `MeshContainer::open` uses, so every catalog row
  and every upload payload sees the whole table. `RawMesh` keeps its published
  shape byte for byte; the only change to it is that three doc comments that
  described the first-group mirroring as the whole truth now say what it is: a
  view of group `0`, with the group list as the authority.
* **Option (c), a versioned replacement for `RawPolygon`** — not needed and not
  taken. Nothing had to change shape, so there is nothing to version and no
  migration to write.

The F10-A contract is untouched: `RawMesh`, `RawPolygon` and `RawCorner` have
the same fields, the same meaning and the same derives. The reader is untouched
apart from documentation.

## 3. What changed

### `crates/cs_formats/src/gamez/` (documentation and one re-export)

The reader already kept every group. Three doc comments described the
first-group mirroring as a permanent property of the render mesh, which is now
false, and they were the only place a consumer could learn that the group list —
not `RawPolygon::material` — is the authority:

* `RawPolygon::material` and `RawCorner::uv` now say they are material group `0`,
  and say what `RawPolygon::material` is for a polygon that stored no group at
  all (the reader's `0`, a value the bytes never said).
* `GameZMesh::material_groups` now says the list, not the IR field, decides how
  many groups a polygon has, and carries the per-archive measurement.
* `GameZMesh::corner_uv` is now described as the lookup a multi-group polygon
  needs, rather than as a workaround.
* `RawMaterialGroup` is re-exported from `cs_formats::gamez`, which
  `cs_content` needs to name the type in a public signature.

No behaviour in `cs_formats` changed, and no `cs_formats` test needed to change.

### `crates/cs_content/src/mesh.rs`

* **`RenderMesh::from_stored_groups`** (and `…_with_topology`) is the new input.
  A polygon that stored `n` groups produces `n` render triangles per topology
  triangle, one per group, each with that group's own raw material index and
  that group's own UV set. `RenderTriangle::group` and `RenderVertex::group` say
  which group a triangle or vertex is.
* **The vertex key is unchanged in shape and now keyed on the group's own
  values.** The group *index* is deliberately **not** in the key: two groups
  that store the same material and the same coordinate for a corner are the same
  vertex, and the triangles reaching it say which group they are. What is in the
  key is the resolved `(position, normal, uv, color, material)`, so a group whose
  UV differs splits. This is the split the task asks for.
* **`RenderMesh::build` / `from_parts` stay** and are now documented as the
  *single-group* reading: exact for a mesh whose polygons each stored one group,
  and a first-group view of a multi-group one. They share one implementation
  with the group-aware path through a private `GroupSource`, so there is no
  second splitter that could drift and no branch that can drop a group.
* **Three named refusals** for a group table that does not describe the mesh:
  `GroupCount`, `GroupCornerCount` and `PolygonWithoutMaterialGroup`, with
  codes `material_group_count`, `material_group_corner_count` and
  `polygon_without_material_group`. See "The refusals, and what reaches them".
* **`RenderMesh` counts the groups' own draws**: `extra_group_triangles()` and
  `extra_group_degenerate_triangles()`. `source_triangles()` and
  `degenerate_triangles()` are **draw** counts and are therefore the stored
  topology's counts *plus* those; the two accessors make the relation exact for
  any consumer, and a row's `MeshFaceCounts` still describes the **stored**
  mesh, which is what AC04 asks for.
* **`MeshPresentationUnknown::MultiMaterialGroup`**, code
  `multi_material_group_presentation_unknown`, replaces the retired
  `multi_material_group_polygons` reason. It is on a row and a payload **only**
  when that mesh really stored a multi-group polygon.

### Why drawing every group is a lossless reading and not a guess

A stored polygon with two groups carries two authored UV sets over the same
corners and two authored material indices. Carrying all of them into the render
mesh says nothing about what to draw: it states what the bytes say. Deciding
which group the original drew, in what order, and whether a later group covers an
earlier one is a *presentation* decision, it is unmeasured, and it is exactly
what `MeshPresentationUnknown::MultiMaterialGroup` names. Drawing all of them is
therefore a faithful reading, not a claim about the original, and the claim is
blocked on a row rather than made silently.

## 4. The reason change, and why the old code had to go

F10-C put `multi_material_group_polygons:<count>` on every row whose mesh stored
a multi-group polygon, and the parent stage then made it a bare
`multi_material_group_polygons`. **That code named a loss**: its own doc said the
render mesh carries the first group only, so a later group has no UV set here.
The loss is gone. Keeping the code would mean a row asserting a defect that no
longer exists, and a consumer grouping rows by it would count meshes for a
reason that is not a reason any more. It is removed, not renamed.

What is left, on the same rows, is the open presentation question. Both pieces
of evidence stay where a consumer can find them, and neither is folded into a
code: the polygon count on
`MeshFaceCounts::multi_material_group_polygons` and the extra draws in
`RenderMesh::extra_group_triangles()`. That is the same code-versus-evidence
split F10-C established for the container-duplicate name.

**No stored material group is lost without a named row reason.** The two ways
that could happen are both closed and both tested: a later group that reaches
neither the render mesh nor the payload (the F10-E acceptance test), and a
polygon that stored *no* group at all, where the row names
`polygon_without_material_group` and nothing is uploaded for that mesh.

## 5. The refusals, and what reaches them

| Refusal | Reachable from | Measured on the corpus |
| --- | --- | --- |
| `GroupCount` | a hand-built group table | never: the reader builds the table beside the polygons |
| `GroupCornerCount` | a hand-built group table | never: the reader reads `mat_count` × `corners` coordinates |
| `PolygonWithoutMaterialGroup` | **the production reader**, on a polygon storing `mat_count == 0` | never: F10-B's whole-corpus test asserts `parsed.findings.is_empty()`, and `PolygonWithoutMaterial` is one of its findings |

The first two are guard clauses on a public entry point, stated and tested
because a caller can hand a table the mesh does not describe, and they are
recorded here as never having fired rather than left implied. The third is a
real decision: the reference asserts `mat_count > 0`, and the two ways of getting
on with `mat_count == 0` are both worse than a refusal — drawing the face with a
material the bytes never named, or dropping the face. So the mesh is **refused**,
the refusal is a row with its code, its diagnostic, `offset: None` (a check over
bytes read whole invents no offset) and its exact face counts, `resolve` refuses
with the same code, and nothing is uploaded. It is reached in
`accept_f10_e_a_polygon_without_a_stored_group_is_refused_by_name` through a
synthetic container that really stores `mat_count == 0`.

## 6. Retail evidence: `ZBD/C1/gamez.zbd`

`accept_f10_e_retail_world_multi_group_polygons_keep_every_stored_group` runs the
whole production path on the original installation — `install::discover`,
`SessionBuilder::mount_installation`, `ZbdContainer::open`, both readers,
`RenderMesh`, `MeshCatalog::prepare_upload` — over the world with the most
multi-group polygons. Measured, all asserted as invariants:

| | value |
| --- | --- |
| rows (stored meshes) | 2 237 |
| rows holding a multi-group polygon | 61 |
| multi-group polygons | 390 (5 of them store three groups) |
| extra draws the later groups add | 1 042 |
| polygons whose groups agree on every coordinate | 8 |
| positions split by a coordinate set only a group beyond the first holds | **1** |
| rows failing the render gate | 0 |

The test asserts the *relations* that only hold when nothing is lost and nothing
is invented — `source_triangles() == stored triangles + extra_group_triangles()`,
`degenerate_triangles() == stored degenerate + extra_group_degenerate`, and for
every multi-group polygon and every one of its groups, that the payload's
vertices sample that group's own coordinate for that group's own source corner
and carry that group's own material. Those figures are a floor, not a checksum:
pinning exact numbers would make the test a digest of one owner's copy, which is
what F10-C's integration stage already decided and recorded.

**What this test does not claim.** It does not claim any group *looks* right,
that a seam is drawn where the original drew one, that the original drew one
group or all of them, or that any of the four presentation unknowns is settled. A
passing run is `checked`, not `verified_original`: these are measurements of
bytes through production code, not observations of original behaviour.

The per-archive table at the top is not re-measured by this test. F10-B's
`accept_f10_b_gamez_retail_flags_groups_and_seams_over_the_whole_corpus` walks all
nine archives and already asserts the group histogram, that multi-group polygons
exist, that at least one stores genuinely different coordinates per group, and
that `ZBD/planes.zbd` stores none. Re-measuring the same nine archives from
`cs_content` would duplicate it at the cost of a second installation mount.

## 7. Test inventory

All in the `#[cfg(test)]` module of `crates/cs_content/src/mesh.rs`, all
`accept_f10_e_`:

| Test | What fails if the behaviour is removed |
| --- | --- |
| `..._every_stored_material_group_reaches_the_upload_payload` | The whole group path. A first-group-only build, a build that pairs a group's geometry with the first group's material, or one that draws a group once instead of once per topology triangle. Also the exact triangle, extra-draw and degenerate arithmetic against the stored face counts. |
| `..._second_group_uv_seam_stays_a_visible_seam` | **AC03 for the second group.** A splitter keyed on the position index alone, one that welds a group's corners together, or one that drops the group's material from the key all fail here. The fixture's *first* group has no seam at the position, so a pass cannot be an accident. |
| `..._a_multi_group_row_names_the_open_question_and_a_single_group_row_does_not` | The reason contract. The retired code on any row or payload, the presentation question on a mesh that stored no multi-group polygon, the question missing from one that did, or a count folded back into a code all fail. |
| `..._source_maps_and_degenerates_survive_every_group` | F10-C.01's maps after a new keyed attribute: a triangle map that is not the stored topology times the stored groups, a vertex naming a corner or a group that does not exist, per-group degeneracy that differs between groups, or a material grouping that is not a partition. |
| `..._a_polygon_without_a_stored_group_is_refused_by_name` | The refusals. Inventing a group for a `mat_count == 0` polygon, dropping the face silently, or indexing a table that does not describe the mesh all fail; so does a row that loses the refusal's code, its diagnostic or its exact face counts. |
| `..._retail_world_multi_group_polygons_keep_every_stored_group` (`#[ignore]`) | The whole path on the original installation, and the AC03-for-the-second-group case on real data. |

### Sensitivity probes actually run

Each is a one-line mutation of the production code, run against
`cargo test -p cs_content --lib -- accept_f10_e_` (the retail test excluded: a
probe that only changes an assertion would also "pass" without the fix), then
reverted:

| Mutation | Tests that failed |
| --- | --- |
| `MeshContainer::open` back to `RenderMesh::build` (the F10-C build) | all five |
| `VertexKey::material` takes the polygon's first-group material instead of the group's | the group-payload test, the second-group-seam test |
| The later groups' **draws** dropped while their vertices are still built | the group-payload, second-group-seam, source-map and reason tests |
| `MultiMaterialGroup` pushed on **every** payload, group or not | the reason test **and** `accept_f10_c_03_uv_seam_survives_the_container_to_upload_boundary` |
| `check_group_table` quietly accepts a polygon with no group | the refusal test |

The last two are the two halves of this task's contract: the reason must be on
the rows where the question is real *and* on none of the others, so a probe that
breaks either direction has to fail. The fourth probe's second failure —
`accept_f10_c_03_uv_seam_survives_the_container_to_upload_boundary`, a
pre-existing F10-C test that never mentioned groups — is an independent witness
that the row contract is checked by more than the test written for it.
## 8. Recorded unknowns and open limitations

* **How the original renderer presented a multi-group polygon is unmeasured** —
  one group or all, in what order, whether a later group covers an earlier one.
  `MeshPresentationUnknown::MultiMaterialGroup` says so on every row and payload
  whose mesh stored such a polygon, and it gates `Ready` like the other three.
* **What a material group *means* is still not established.** The layout stores
  the groups; whether the intent is a decal, a second surface layer, a cockpit
  interior, or something else is not measured here and was not guessed. What is
  established is only that the groups carry distinct material indices and
  distinct coordinates, and that the render mesh now carries all of them.
* **No row can be `Ready`** while the four presentation unknowns are open, so no
  render-mesh fidelity claim changes state because of this stage.
* **The front-face winding, the UV origin convention and the corner-colour
  meaning are untouched** and still unknown; see
  `docs/findings/2026-09-29-f10-c-03-mesh-container-catalog-and-upload.md`.
* **`GroupCount` and `GroupCornerCount` have never fired** and cannot be reached
  from the reader. They are guard clauses on a public entry point, tested
  directly, and recorded as unobserved rather than left implied.
* **A polygon that stores no group costs its whole mesh.** One face with
  `mat_count == 0` refuses the render mesh it belongs to, so its sibling faces
  are not uploaded either. The alternative — dropping one face — is what the spec
  forbids in the other direction, and the reference asserts the case does not
  occur. Measured on the corpus: it does not.
* **No renderer has drawn any of this.** F17-B's adapter is the consumer, and it
  reads `MeshUpload::unknowns` before it chooses anything. It now has four
  unknowns to read, one of them new.
* **The spec sheet has no `### F10-E` section.** `specs/F10-…` ends at `### F10-D`,
  and `specs/` is a protected path, so this stage's authority is the task
  description. The test prefix follows the task key (`accept_f10_e_`) and every
  acceptance criterion of the parent sheet — AC01 to AC04 — is preserved and
  re-asserted; the sheet is unchanged and should gain a section for F10-E and
  for F10-D, which this task did not touch.
* **Per-archive airframe behaviour is a zero, not an absence.** `ZBD/planes.zbd`
  stores no multi-group polygon, so the airframe producer never exercises the
  group path at all. A future Crimson Skies asset with one would be the first
  real test of the presentation unknown on an airframe, and this stage says so
  rather than claiming the path is covered there.

## 9. Sources

* [S02], [S03], [S08] in `docs/research/SOURCES.md` — the pinned reference the
  layout came from, via F10-B and F10-C.02.
* `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` — the mesh-section
  worksheet, the `PolygonMaterialNg` reading and F10-B's own first measurement
  of the multi-group counts.
* `docs/findings/2026-09-29-f10-c-01-render-vertex-splitting.md` — the split, the
  source maps and the degenerate accounting this stage keeps.
* `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` — the audit a
  group's material index reaches.
* `docs/findings/2026-09-29-f10-c-03-mesh-container-catalog-and-upload.md` — the
  container, the catalog and the upload boundary, and the `multi_material_group`
  reason this stage retired.
* `docs/findings/2026-09-29-f10-c-integration-and-reason-codes.md` — the
  reason-as-code contract and the airframe measurement that predicted
  `planes.zbd`'s zero.
* `docs/contracts/IDENTITY-CONTENT.md` — the catalog element fields and the
  "collections cannot exclude failed entries" rule.

No code was copied from any reference; mech3ax is EUPL-1.2 and is read as a
reference only. No original game data is committed, and the retail test reads
`$CS_GAME_DIR` only.
