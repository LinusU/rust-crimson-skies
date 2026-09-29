# F10-D: the exact face census of the private geometry corpus

**Task:** #44 / `F10-D` — *Close every render-critical unknown on the private
geometry corpus* (`specs/F10-gamez-mesh-topology-and-material-records.md`,
section `### F10-D`, AC04).
**Capabilities used:** `retail` (read access to `$CS_GAME_DIR`) and ordinary
build/test. No original executable was run, no GPU, no audio, no human play.
**Claim class:** `implemented`, layout/content class `ObservedTool`. Reading
original *files* is not evidence of how the original *behaved*; nothing here is
`verified_original`.
**Contract:** `docs/contracts/IDENTITY-CONTENT.md`, `docs/contracts/CLI-EVIDENCE.md`.

## What F10-D had to answer

AC04: **"Report exact missing/invalid face counts for every private world and
airframe."** F10-B's deferred table handed four items to this stage: the
`unk` words (#4), front-face winding (#6), the nine outlines this triangulator
refuses plus planarity (#7), and the zero-length mesh record the corpus never
contains (#11). This document records what the corpus closed and what it cannot
close without an original run.

## Definitions (the production code says the same)

`cs_formats::gamez::FaceCensus` (`crates/cs_formats/src/gamez/census.rs`) counts,
per container:

| word | exact meaning |
| --- | --- |
| **declared** | `polygon_count` summed over the present mesh records — what the records say the container holds |
| **stored** | polygon records actually read (`declared - stored` is the *shortfall*) |
| **invalid** | stored faces whose data fails validation: index past its array, too few corners, non-finite value (`FaceIssue` that is not `UnsupportedNgon`) |
| **unsupported** | stored faces that are valid data this triangulator refuses to guess at (an n-gon outline it cannot triangulate) |
| **degenerate-only** | faces that decoded but whose triangles are all degenerate: they draw nothing |
| **missing** | `shortfall + invalid + unsupported + degenerate-only`: every declared face that reaches **no drawable triangle**. `invalid ⊆ missing` |
| **faces in failed rows** | at the render gate: every stored face of a mesh that produced no render mesh, i.e. the faces that will not be drawn *at all* |

The last row is the render-critical one: one refused face costs its whole mesh
at `RenderMesh::build`, because the gate refuses an incomplete topology rather
than dropping faces (`RenderMeshError::IncompleteTopology`).

## The report: every private world and airframe

Format layer — `accept_f10_d_retail_every_world_and_airframe_reports_exact_missing_and_invalid_face_counts`
(discovery derives the archive list: every observed world group plus
`planes.zbd`):

| archive | kind | slots | present | absent | declared = stored | missing | invalid | unsupported | degenerate-only | triangles | of which degenerate |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `ZBD/C1/gamez.zbd` | world | 2 250 | 2 237 | 13 | 18 277 | 0 | 0 | 0 | 0 | 56 073 | 8 019 |
| `ZBD/C1B/gamez.zbd` | world | 1 500 | 1 305 | 195 | 8 323 | 0 | 0 | 0 | 0 | 22 063 | 2 300 |
| `ZBD/C1C/gamez.zbd` | world | 2 250 | 1 518 | 732 | 8 040 | 0 | 0 | 0 | 0 | 21 759 | 2 267 |
| `ZBD/C2/gamez.zbd` | world | 2 250 | 1 765 | 485 | 12 645 | 0 | 0 | 0 | 0 | 39 618 | 5 816 |
| `ZBD/C2B/gamez.zbd` | world | 1 500 | 1 365 | 135 | 7 008 | 0 | 0 | 0 | 0 | 18 539 | 1 795 |
| `ZBD/C3/gamez.zbd` | world | 2 250 | 1 901 | 349 | 16 087 | 0 | 0 | 0 | 0 | 44 683 | 5 532 |
| `ZBD/C4/gamez.zbd` | world | 3 000 | 2 431 | 569 | 19 661 | **3** | **0** | **1** | **2** | 69 851 | 13 042 |
| `ZBD/C5/gamez.zbd` | world | 3 000 | 2 851 | 149 | 22 493 | **8** | **0** | **8** | **0** | 68 258 | 9 837 |
| `ZBD/planes.zbd` | airframe | 2 250 | 1 766 | 484 | 16 200 | 0 | 0 | 0 | 0 | 71 645 | 10 497 |
| **total** | | 20 250 | 17 139 | 3 111 | **128 734** | **11** | **0** | **9** | **2** | 412 489 | 59 105 |

Render gate — `accept_f10_d_retail_render_gate_drops_exactly_the_refused_faces_of_every_archive`
(`cs_content::mesh`, same nine archives through the production VFS, dispatch,
readers and `MeshCatalog`):

| archive | rows | ready | blocked | failed | faces | faces missing from the render |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `ZBD/C1` … `ZBD/C3` | 10 091 | 0 | 10 091 | 0 | 70 380 | 0 |
| `ZBD/C4` | 2 431 | 0 | 2 430 | **1** | 19 661 | **59** |
| `ZBD/C5` | 2 851 | 0 | 2 843 | **8** | 22 493 | **30** |
| `ZBD/planes.zbd` | 1 766 | 0 | 1 766 | 0 | 16 200 | 0 |
| **total** | **17 139** | 0 | 17 130 | **9** | **128 734** | **89** |

Read the two totals together: **11** faces of the corpus draw nothing, spread
over **10** meshes. Nine of those meshes fail the render gate — every refused
face does, because the gate refuses an incomplete topology — so **89** stored
faces, every face of those nine meshes, never reach a renderer. The tenth
(mesh 192 of C4) lost only a degenerate-only face, which decodes and therefore
still builds. That ratio is why "render-critical" is a stronger statement than
"eleven broken faces": the smallest possible defect — one refused outline in
C4's mesh 191 — costs 59 faces of world geometry.

No row is `Ready`: every row carries at least one open presentation or material
reason (F10-C's finding), which is reported, not counted as missing — a
`Blocked` row still has its render mesh.

## The eleven faces, by identity

Stored indices and stable reason codes only — never a coordinate, a name or a
byte of the archives. Pinned by
`accept_f10_d_retail_every_world_and_airframe_reports_exact_missing_and_invalid_face_counts`
and re-derived independently by the evidence harness.

| archive | mesh | polygon | corners | flag byte | reason |
| --- | ---: | ---: | ---: | ---: | --- |
| `ZBD/C4` | 191 | 20 | 5 | `0x10` | `coincident_corners` (corners 0 and 1) |
| `ZBD/C4` | 191 | 55 | – | – | `degenerate_only` (its only triangle repeats a position index) |
| `ZBD/C4` | 192 | 142 | – | – | `degenerate_only` |
| `ZBD/C5` | 370 | 1 | 5 | `0x00` | `coincident_corners` (corners 3 and 4) |
| `ZBD/C5` | 970 | 0 | 17 | `0x44` | `self_intersecting` (edges 4 and 16) |
| `ZBD/C5` | 973 | 0 | 19 | `0x44` | `self_intersecting` (edges 1 and 18) |
| `ZBD/C5` | 974 | 0 | 24 | `0x44` | `self_intersecting` (edges 5 and 23) |
| `ZBD/C5` | 978 | 0 | 30 | `0x44` | `self_intersecting` (edges 23 and 29) |
| `ZBD/C5` | 980 | 0 | 36 | `0x44` | `self_intersecting` (edges 0 and 18) |
| `ZBD/C5` | 981 | 0 | 124 | `0x44` | `self_intersecting` (edges 6 and 123) |
| `ZBD/C5` | 982 | 0 | 124 | `0x44` | `self_intersecting` (edges 0 and 18) |

Three observations, each a fact about stored bytes:

* the two `coincident_corners` outlines visit the same location twice; the
  reader keeps both corners (each owns its own UV/colour) and the triangulator
  refuses rather than deleting one;
* the six `self_intersecting` outlines are the largest in the corpus (17–124
  corners) and all carry flag byte `0x44`, i.e. `FLAG_NORMALS | FLAG_UNK6 |
  FLAG_UNK2`; the crossing is reported *in the projected outline*, which is the
  plane the triangulator chose from the Newell normal;
* **no** face of the corpus is invalid: `invalid = 0` in all nine archives, so
  the missing faces are all either refused valid data or degenerate draws.

## Planarity of retail n-gons — the second half of deferred item 7, measured

`accept_f10_d_retail_ngon_outlines_are_measured_for_planarity` measures, for
every stored outline with more than three corners, the largest corner distance
from the outline's own Newell plane (exact `f64` arithmetic over the stored
`f32`s, no tolerance). The two bins are **reporting thresholds this task
authors** so the measurement is reproducible; they are not game values.

| archive | outlines | exactly planar | > 1e-6 world units | > 1e-3 | max deviation (world units) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `ZBD/C1` | 12 163 | 11 519 | 644 | 164 | 8.86e1 |
| `ZBD/C1B` | 5 820 | 5 503 | 317 | 93 | 6.84e1 |
| `ZBD/C1C` | 5 924 | 5 618 | 306 | 93 | 8.72e-2 |
| `ZBD/C2` | 8 587 | 8 048 | 539 | 123 | 4.12e1 |
| `ZBD/C2B` | 5 353 | 5 119 | 234 | 72 | 5.12e-2 |
| `ZBD/C3` | 9 336 | 8 719 | 617 | 174 | 7.62e-2 |
| `ZBD/C4` | 11 008 | 9 533 | 1 475 | 250 | 5.85e1 |
| `ZBD/C5` | 15 908 | 15 312 | 596 | 182 | 4.09e2 |
| `ZBD/planes.zbd` | 8 968 | 8 028 | 940 | 176 | 9.57e-2 |
| **total** | **83 067** | **77 399** | **5 668** | **1 327** | |

**The unknown is closed as a measurement:** retail n-gons are *not* all planar —
5 668 of 83 067 stand off their own plane in every archive, by up to 409 world
units in C5. That is why `triangulate_polygon` projects onto the dominant plane
instead of assuming coplanarity, and why "is it planar?" is no longer an open
question. What the *original renderer* did with an outline that is not planar
remains an original-run question (below).

## What this stage closed, and what it cannot close from files

Closed on the corpus:

1. **AC04 itself.** Exact missing/invalid counts for all nine archives, with the
   identity of every missing face, cross-checked three ways: the records'
   declared counts against what the section held, every stored face counted
   exactly once (`decoded + invalid + unsupported = stored`), and the render
   gate's own verdict agreeing archive by archive with the census.
2. **Planarity of retail n-gons** (F10-B deferred item 7, second half) —
   measured, tabulated above.
3. **The visible consumers of the refused faces** (F10 non-negotiable #4): the
   nine meshes the render gate drops, their face counts, and the fact that no
   other mesh loses anything.
4. **Item 11 (the zero-length mesh record deviation) stays decided *by* the
   corpus:** all 17 139 present records store data, so the corpus still cannot
   choose between this reader's strict upper bound and the pinned reference's
   inclusive one. That is now a *measured* impossibility rather than an
   unchecked assumption: `shortfall = 0` in all nine archives and the deviation
   is pinned by F10-B's `..._a_meshes_declared_offset_must_match_the_walk`.

Not closable with `retail` (files are not a run of the executable):

* what the original engine drew for the eleven faces above — fanning them,
  skipping them or refusing them is unobservable from files;
* front-face winding and handedness (deferred item 6);
* the meaning of every stored `unk` word (deferred item 4) — nothing here is
  read as specularity, soil or a colour (F10 non-negotiable #5);
* whether a stub mesh slot is referenced by a node (3 111 absent slots across
  the nine containers) — F11-A owns node records.

## Deferred scope, its resolving task and what it gates

The acceptance report's `unknowns` array is **empty**. That is a statement, not
an omission: everything this stage could not resolve is a named boundary with a
resolving task, recorded here — the durable, versioned record that outlives the
report. Nothing was removed from the report to make a validator pass.

| # | Deferred item | Affected content | Resolving task | Gates |
| --- | --- | --- | --- | --- |
| 1 | What the original renderer drew for the **eleven faces** that reach no drawable triangle, and for the nine meshes the gate drops | C4 mesh 191 (polygons 20 and 55) and 192 (polygon 142); C5 meshes 370, 970, 973, 974, 978, 980, 981, 982 — 89 of 128 734 faces will not render until it is answered | **filed with `create_tasks` as an original-run question (owner)**: needs `human_review`/an original capture, which no agent has | any claim that world geometry of C4/C5 is complete; `verified_original` for the mesh path |
| 2 | **Front-face winding and handedness** (F10-B item 6) | every polygon of every mesh in all nine archives | **filed with `create_tasks` as an original-run question (owner)**; the render adapter that consumes the winding is F17-B | any lighting, culling, normal or two-sided-material claim |
| 3 | The meaning of every stored `unk` word (F10-B item 4), including the six refused outlines' shared flag byte `0x44`'s `FLAG_UNK6`/`FLAG_UNK2` bits | header `unk08`/`light_index`, mesh record `unk04/unk08/unk40/unk44/unk72/unk76/unk80/unk84`, polygon `unk04/unk28/unk32/unk36`, every `LightC` field | F11-A (nodes) and F18 (lights) where a field turns out to be needed | any claim that a stored value has a *meaning*; nothing may be read as specularity or soil (F10 non-negotiable #5) |
| 4 | The **zero-length present mesh record** (F10-B item 11): this reader's bound excludes `nodes_offset`, the pinned reference's is inclusive, and no measured archive contains such a record | a present mesh record storing no data at all; none of the nine archives | F10-D measured that the corpus cannot decide it (`shortfall = 0` everywhere); the only place it can appear is an unmeasured container | any claim that this reader accepts exactly the reference's containers |
| 5 | Whether a **stub mesh slot** is reachable from a node (3 111 absent slots) | every absent slot of the nine archives (484 in `planes.zbd`, 732 in C1C, …) | F11-A (node → mesh addressing) | any claim that a world renders everything its nodes reference |
| 6 | Whether the **original renderer** did any of the above at all — how the engine consumed this section | the whole mesh section | the owner (`human_play` / `human_review`) | every `verified_original` and `release_approved` claim; this report's claim is `implemented` |

## Production code and tests added

* `crates/cs_formats/src/gamez/census.rs` — `FaceCensus`, `MissingFace`,
  `MissingFaceReason`, plus `GameZMeshes::face_census()` in `reader.rs` and the
  module wiring in `mod.rs`. The census walks `RawMesh::topology`, the same
  gate the upload path walks; it adds no second opinion about any face.
* `crates/cs_formats/tests/gamez/d.rs` — four task tests:
  * `accept_f10_d_census_names_every_missing_face_with_its_exact_reason`
    (synthetic: invalid / unsupported / degenerate-only / absent stub, every
    count and every listed face);
  * `accept_f10_d_census_counts_a_face_the_section_never_held` (synthetic
    failure case: a record declaring faces the section never held, and its
    complete contrast);
  * `accept_f10_d_retail_every_world_and_airframe_reports_exact_missing_and_invalid_face_counts`
    (retail, AC04);
  * `accept_f10_d_retail_ngon_outlines_are_measured_for_planarity` (retail).
* `crates/cs_content/src/mesh.rs` — `accept_f10_d_retail_render_gate_drops_exactly_the_refused_faces_of_every_archive`
  (retail; mounts all nine archives through the production path, asserts the
  two layers agree, and writes `render-gate.tsv` when `CS_EVIDENCE_DIR` is set).
* `crates/cs_formats/tests/gamez/evidence.rs` — the evidence harness
  (`evidence_report_f10_d_writes_the_acceptance_report`), deliberately not named
  `accept_f10_d_*`.

Synthetic tests run in CI; the three retail tests are
`#[ignore = "requires CS_GAME_DIR"]` and **fail loudly** when `CS_GAME_DIR` is
absent rather than skipping.

## Reproducing the evidence

```sh
mkdir -p private/evidence/F10-D
CS_EVIDENCE_DIR=private/evidence/F10-D \
cargo test --workspace --locked -- accept_f10_d_ --include-ignored \
  2>&1 | tee private/evidence/F10-D/cargo-test.log

CS_EVIDENCE_DIR=private/evidence/F10-D \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f10_d_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=<status of the run above> \
  cargo test --locked -p cs_formats --test gamez evidence_report_f10_d -- --ignored

python3 tools/validate_evidence.py \
  private/evidence/F10-D/acceptance.json \
  --artifact-root private/evidence/F10-D --require-pass

cp private/evidence/F10-D/acceptance.json docs/findings/evidence/F10-D.json
```

Artifacts (hashed, never committed): `cargo-test.log`, `mesh-face-census.json`
(per-archive census, digests and the identity of every missing face),
`render-gate.tsv` (the gate's own measured verdict). The harness re-derives
each archive's census from the production reader and **refuses any disagreement
with `render-gate.tsv`**, so neither side of the report can be produced alone.

## Sources and honest limits

* The layout and the reader come from F10-B's worksheet against the pinned
  mech3ax CS-capable revision (`docs/research/SOURCES.md` S02/S03/S08; commit
  `d3521a9721be731d365504568ddcd78e3f9846bb`, read only, no code copied).
* Every number above was produced by running the production reader and census
  over the original installation on this machine; the report's
  `install_sha256`/`content_sha256` come from F02's production fingerprint.
* `retail` means file access. No original executable ran, so no statement here
  describes the original's runtime behaviour, and no agent review replaces the
  owner's approval.
