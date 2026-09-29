# F10-C.03: the GameZ container, the render-mesh catalog and the upload boundary

**Task:** #366 (`F10-C.03`), slice of Rally #43 / `### F10-C` of
`specs/F10-gamez-mesh-topology-and-material-records.md`.
**Shared contract:** `docs/contracts/IDENTITY-CONTENT.md`.
**Test prefix:** `accept_f10_c_03_`.
**Capabilities used:** ordinary build/test, and `retail` for the one
`#[ignore = "requires CS_GAME_DIR"]` test.

## What the task asked for, and where each part landed

The stage's own words: *"Wire the implemented path into its actual producer and
consumer; include teardown/retry and error propagation."* F10-C.01 produced a
[`RenderMesh`] and F10-C.02 produced a [`MeshDependencyAudit`], and both were
reachable only from a hand-built struct. Nothing in the repository opened a
GameZ container through a content session, and nothing handed a render mesh to a
consumer.

| Asked for | Landed as |
| --- | --- |
| Producer: open a GameZ/PLANES container through a content session (F04 VFS + F06-C `ZbdContainer`, checking the family) and read its meshes with #363's reader | `MeshContainer::open` |
| Content: build F10-C.01 render meshes plus F10-C.02 material audit | `MeshContainer::open` builds one `RenderMesh` per present slot and one `MeshDependencyAudit` per container |
| Catalog rows per mesh: kind `render_mesh`, origin span, dependencies, parse/normalize state, readiness, unsupported reasons, fingerprint | `RenderMeshRecord`, produced by `MeshCatalog::records` |
| Failed meshes **and containers** stay rows | a refused mesh is a row with `failure`; a refused container is a row with `id: None`, `mesh_index: None` |
| Consumer boundary: an upload payload that owns its data and survives session close | `MeshUpload` |
| A thin `crates/cs_app/src/mesh.rs` adapter only if it does not pre-empt F17-B | **not added** — see "Why there is no `cs_app/src/mesh.rs`" |
| Stale state: a foreign session or catalog is refused | `MeshError::ForeignSession`, `MeshError::NotFromThisCatalog` |
| `retry_failed` reopens failed containers in the same session; a remount loads a repaired file | `MeshCatalog::retry_failed` |
| Error propagation: reader errors keep their contextual offset/member through to the catalog row | `MeshFailure { container, member, offset }` |

## The path, end to end

```
AssetKey  ->  ContentSession::resolve + ZbdContainer::open
          ->  family == ZbdFamily::GameZ ?          (else WrongFamily)
          ->  read_gamez_meshes   (mesh section, ends at nodes_offset)
          ->  read_gamez_materials(material section, ends at meshes_offset)
          ->  the two 40-byte headers must agree     (else HeaderDisagreement)
          ->  RenderMesh::build per present slot    (Err kept in the slot)
          ->  MeshDependencyAudit::build            (F10-C.02, unchanged)
          ->  RenderMeshRecord per stored mesh / per failed container
          ->  MeshCatalog::resolve + prepare_upload
          ->  MeshUpload { render, materials, faces, origin, unknowns }
```

Every step is production code. The synthetic fixtures in the test module author
whole GameZ containers from the layout worksheets; they share no code with
`cs_formats::gamez` and every expected value is a literal, so a writer and a
reader that made the same mistake cannot agree.

## Design decisions

### Two section readers, one container, and a header cross-check

`read_gamez_meshes` and `read_gamez_materials` each prove their own section
boundary, on purpose, so a caller that wants both reads the bytes once per
entrypoint. `MeshContainer::open` therefore runs both against the **same**
`ZbdContainer`'s bytes with the **same** `ParseContext` (whose container label is
the VFS's, `"<mount> at <key>"`), and then compares all ten header words the two
readers produced. A disagreement is refused as
`MeshContainerErrorKind::HeaderDisagreement`.

The reader arguments that name a container are ignored by both readers on
purpose — "the container label is the parse's own, and a reader must not carry a
second, possibly different one" (`read_gamez_meshes`). The label is therefore
taken from the `ZbdContainer`, not from the caller's key spelling, so the label a
row and a failure report is the one the VFS and the readers both used.

### A refused mesh does not fail its container

A stored mesh whose faces do not survive `RenderMesh`'s validation gate is kept
as `Some(Err(..))` in its own array slot. A sibling mesh of the same container
may be complete, and dropping the whole container because one face of one mesh
is broken would be exactly the "silently drop broken faces" the spec forbids in
the other direction. The refused mesh becomes a **row** with
`readiness: Failed`, its fingerprint (its stored span *was* walked), the exact
face counts, and the gate's own message naming every rejected face and its
`FaceIssue` code. `resolve` refuses it with `MeshError::MeshFailed`, so nothing
is uploaded for it.

An **absent** array slot is different: it stores no mesh, so it is not a row at
all. The contract's "collections cannot exclude failed entries" is about entries
that exist and did not work; a stub slot is not an entry. `resolve` refuses it
with `MeshError::MeshNotFound`, and it is never filled from a sibling.

### Rows: what each field means here

* `kind` is always `"render_mesh"` (IDENTITY-CONTENT: "render meshes/materials/images").
* `dependencies` is **two** keys, always: the container itself, and the one
  texture archive the audit searched. A render mesh's material index is a stored
  number whose only origin is a texture in a named archive, so a mesh row that
  named only the container would be understating its own closure.
* `fingerprint` is SHA-256 over `mesh.data_offset..mesh.data_end` — exactly the
  span the reader walked for that mesh, not the whole container and not the
  re-derived render mesh. It is therefore available for a refused mesh too, and
  two meshes of one archive differ if and only if their own stored bytes differ.
* `parse_state` is `Parsed` for a stored mesh whose bytes were read, *including*
  one the render gate refused; the refusal is in `normalize_state` and in
  `failure`, because the failure is not a parse failure.
* `runtime_consumers` is `["mesh_upload"]`.
* A container that produced no mesh at all has `fingerprint: None` and no face
  counts, exactly as `ImageRecord` gives a failed archive.

### Readiness is three-valued, and `Ready` is not reachable yet

`RenderMeshReadiness` is `Ready | Blocked | Failed`. A mesh row is `Blocked`
while any of these is open, and every one of them is on the row:

1. the mesh's own audit rows' `unsupported_reasons` (F10-C.02's exact-name rule,
   out-of-range material indices, unknown flag bits, an archive the catalog does
   not hold, …);
2. `multi_material_group_polygons:<count>` — a stored polygon that keeps two or
   three material groups. The render mesh carries the **first** group only (the
   IR has one `material` and one per-corner `uv`, and F10-A's shape is a
   published contract this task does not change), so a group beyond the first has
   no UV set in the render mesh. This is reported, never dropped;
3. the three [`MeshPresentationUnknown`] codes.

This mirrors F08-C exactly, where a decoded image with open presentation
questions is `DecodedWithUnknowns` and not `Ready`. **No GameZ mesh row is
`Ready` today, and that is the honest answer** — see the unknowns below.

### The three presentation unknowns

Each is a fact about a value `MeshContainer::open` hands on untouched:

| Unknown | What the code demonstrably does not decide |
| --- | --- |
| `front_face_winding_unknown` | A render triangle keeps the stored winding. Which winding faces the viewer, and therefore which is culled, is unmeasured. Carried from F10-A/F10-B and still open in F10-C.01. |
| `uv_origin_unknown` | A stored texture coordinate is handed over untransformed: no V flip, no wrap, no clamp, no scale. The original renderer's UV convention is unmeasured. The same class of gap as F08-C's `stretch_unknown` and `color_space_unknown`. |
| `vertex_color_unknown` | Three raw `f32`s per corner are handed on. Their range, whether they are intensities at all, and whether the original renderer read them, are unmeasured. |

Related, and deliberately **not** a fourth unknown: stored normals are passed
through unnormalized. F10-C.01 records the render mesh's unknowns as winding and
degenerate handling only, and adding a code the rest of the pipeline does not act
on would be noise. The unnormalized pass-through is documented here and in the
`RenderVertex::normal` field instead.

## Teardown, retry and stale state

Modelled on `cs_content::textures`, deliberately, because the two have the same
shape of problem.

* **Owning the data.** `MeshContainer` holds a `ZbdContainer`, which owns its
  bytes. `MeshUpload` holds a `RenderMesh` (owned vectors), the `MaterialRow`s
  the mesh's own references reach, a `SourceSpan` and a `SessionGeneration`. It
  borrows nothing: `accept_f10_c_03_a_payload_owns_its_data_and_survives_its_session`
  drops the catalog and closes both sessions and then still reads the seam back.
* **Foreign session.** `require_session` compares `SessionGeneration`.
  `resolve`, `prepare_upload` and `retry_failed` all go through it, and the code
  is `foreign_session`. A world switch replaces the session, so a mesh resolved
  for world `c1` is never served to world `c2`.
* **Foreign catalog.** `SessionGeneration` alone is not enough: two catalogs of
  the *same* session over the *same* bytes produce the same generation and the
  same `SourceSpan`, so a `ResolvedMesh` from one would be accepted by the other.
  `MeshCatalog` therefore carries a process-local serial
  (`NEXT_CATALOG_SERIAL`) and `ResolvedMesh` carries it back;
  `prepare_upload` refuses a mismatch as `not_from_this_catalog`. This is a real
  difference from `TextureCatalog`, which has the same latent gap and is not
  changed here — filed as **#381** (`F08-D.02`).
* **Retry.** `retry_failed` reopens only the failed containers of the same
  session and keeps the ones that read. It takes the [`MeshDependencies`] again
  because the audit is rebuilt from the repaired bytes, and the caller — not the
  catalog — owns which texture archive it is rebuilt against.
* **Remount.** Repairing a file does not change what a session mounted: the
  retry then fails on the mount's record of the bytes
  (`ZbdError::Read(..)`), not on the new ones. What loads a repair is a
  **remount** — a new session over the repaired tree and a new catalog over it.
  This is the F08-C behaviour, reproduced deliberately, and it is asserted:
  `retry_failed` twice after the repair still reports one failure, and the
  remounted catalog reads all three meshes.

## Error propagation

`MeshFailure` keeps four things a caller needs and a reworded message would lose:

* `container` — the reader's own parse-context label. A reader's *validation*
  variants name no container (a header is refused on its own words), so the VFS
  label of the container they were handed is the fallback. `<unnamed>` would hide
  which container failed, and a row has to name it.
* `member` — the reader's logical field, already scoped by the parse context, e.g.
  `gamez.meshes.polygon.color.b`. This is `cs_formats::ParseError::field` for a
  truncation; `None` for a check that is not anchored at a field.
* `offset` — the absolute container offset the read anchored at, from
  `GameZError::offset()` / `GameZMaterialError::offset()`.
* `stage` and `code` — which stage refused and the reader's own stable code.

`MeshContainerError` is a struct holding the reason, the origin and the failure
together, so a catalog row **cannot** be built without the context: the two are
constructed in one place and are inseparable afterwards. The three payloads are
boxed so a `Result` carrying the error stays small.

**No offset is invented where no read failed.** A section chain check, a family
refusal and the render mesh's validation gate all report `offset: None`. The
tests assert that the render-gate failure has `offset: None` and the truncation
failures have a specific one, so a future change that fabricated an offset would
fail.

`MeshError::code()` for a failed container is the **reader's** code
(`section_out_of_bounds`, `parse`, `dispatch`, …), not a generic `failed`, so a
caller can tell a truncated file from a missing one. The synthetic test asserts
exactly that.

## Why there is no `crates/cs_app/src/mesh.rs`

The task said to add a thin adapter *"only if it does not pre-empt F17-B
(canonical mesh/image to Bevy adapters); otherwise stop at the content-side upload
boundary, as F08-C did for textures, and record why."*

It would pre-empt F17-B. `specs/F17-rendering-material-fidelity-and-scalable-presentation.md`
gives stage `### F17-B` the title "Implement canonical mesh/image to Bevy
adapters" and owner paths `crates/cs_app/src/render/`,
`crates/cs_app/assets/shaders/`, `crates/cs_app/tests/render/`. A
`cs_app/src/mesh.rs` is outside those owner paths, sits next to the module that
will hold the real one, and would have to decide — in order to be a Bevy mesh at
all — the winding convention, the UV origin and the vertex-colour meaning, i.e.
exactly the three things this stage records as unknown. Writing it now would
either guess them or duplicate F17-B.

So this stage stops at the content-side upload boundary, exactly as F08-C stopped
at [`TextureUpload`], and the boundary is the deliverable. The wiring-only
`lib.rs` doc comment is updated; no other crate changed.

## The family check, and how it is reached

`ZbdContainer::open` refuses a container whose two keys disagree, so
`WrongFamily` is reachable only when the dispatch **succeeds** and routes to a
family that documents no header signature (sound, reader, texture) while the
bytes match no documented signature at all. Two refusals, both tested:

* `rtexture2.zbd` holding a valid texture package → routed to
  `ZbdFamily::Texture` → `MeshContainerErrorKind::WrongFamily`, code
  `wrong_family`, origin present, no stored span walked. The GameZ reader is never
  handed it.
* `rtexture3.zbd` holding a valid GameZ container → the dispatch sees another
  family's documented signature and refuses the keys' disagreement itself
  (`ZbdError::Dispatch`, `HeaderRoleConflict`) → stage `Container`, code
  `dispatch`. A reader is not even chosen.

The reverse — GameZ bytes under `gamez.zbd` — routes to `GameZ` and the GameZ
reader refuses them with its own error, which is the path the truncation and
header-cross-check tests use.

## Truncation: what the reader actually says

Two ways a container can fail to produce meshes, with two different reader
contexts, and both are asserted:

* **The file is cut.** The header's section chain still describes the whole
  container, so `check_sections` refuses it first:
  `GameZError::SectionOutOfBounds { field: "nodes_offset" }` with
  `offset() == Some(nodes_offset)` and no `member` (it is not anchored at a read).
  The row keeps that offset, and the reader's own message — which names the same
  number — is the row's `parse_state` diagnostic verbatim.
* **A mesh record declares more than the section stores.** The chain still
  describes the file, so the reader walks into the mesh data and runs out:
  `GameZError::Parse` with `member == Some("gamez.meshes.polygon.color.b")` and
  `offset == Some(nodes_offset)`. The offset is asserted exactly; the member is
  asserted to be the reader's own scoped field, so a reworded summary would fail.

A third refusal — a face the render gate rejects — has `offset: None` and
`member: None`, because the bytes were read whole. The three are different
contexts for different reasons, and none of them is blurred into the others.

## Retail evidence: `ZBD/C1/gamez.zbd`

`accept_f10_c_03_retail_world_meshes_reach_the_upload_payload` runs the whole
path on the original installation, through `install::discover`,
`SessionBuilder::mount_installation`, `ZbdContainer::open` and both production
readers. Measured facts, all asserted:

* The world's own GameZ container opens: **no** failed container, so both section
  readers accept it and this stage's header cross-check passes.
* `meshes.data_end == nodes_offset` and `materials.data_end == meshes_offset`:
  both readers proved their own section boundary from the same bytes.
* One row per present stored mesh, and **every** row has an id, an origin, a
  fingerprint and exact face counts.
* **Every** row resolves and uploads: no stored mesh in this world failed the
  render gate.
* Each payload's `source_faces`, `source_triangles` and `degenerate_triangles`
  equal the row's counts.
* The split is exactly the authored one: for every upload, two render vertices at
  one position index always differ in one of the four keyed attributes —
  `normal_index`, `uv`, `color` or `material`. Material is per polygon, so a
  shared position across two polygons of different materials is a split in its own
  right, and a check that forgot it would fail here.
* The corpus **does** author per-corner attribute splits, and at least one split
  is a **UV seam** — two vertices at one position index with two different
  authored texture coordinates. That is AC03 on real data, not only on the
  synthetic fixture.

What this test does **not** claim: it does not claim the meshes *look* right,
that any seam is drawn where the original drew one, or that any of the three
presentation unknowns is settled. Those need F17-B's adapter, a renderer, and
F10-D's corpus report. A passing run here is `checked`, not `verified_original`.

The retail test takes about 65 s, essentially all of it the installation mount's
own hashing. That is pre-existing: F10-C.02's retail test in this same module
takes the same 65 s, and F10-B's whole-corpus test in `cs_formats` takes 0.65 s
because it reads the files directly. The path this task added costs
milliseconds on top.

## Recorded unknowns and open limitations

* **Front-face winding / handedness** is unmeasured. `front_face_winding_unknown`
  is on every row and on every payload.
* **The UV convention** is unmeasured. `uv_origin_unknown` is on every row.
* **The corner-colour meaning** is unmeasured. `vertex_color_unknown` is on every
  row.
* **No row can be `Ready`** while those three are open. This gates every
  render-mesh fidelity claim.
* **Multi-material-group polygons lose their extra UV sets** in the render mesh.
  The count is on the row as `multi_material_group_polygons:<count>`; the IR shape
  that cannot hold them is F10-A's published contract, so fixing it is not this
  task's to do. Filed as **#382** (`F10-E`).
* **A `TextureCatalog` of the same session can still answer for a sibling
  `TextureCatalog`'s `ResolvedTexture`**, because the session generation is the
  only binding F08-C has, so F08-C's own doc comment overstates the guarantee.
  `MeshCatalog` does not inherit that gap (it has a serial);
  `TextureCatalog` was not changed here. Filed as **#381** (`F08-D.02`).
* **The dependency audit is rebuilt, not diffed.** A retry replaces a container
  wholesale, so a material that became resolved simply becomes resolved; there is
  no per-row transition history.
* **`retry_failed` needs its dependencies restated.** A caller must still say
  which texture archive the rebuilt audit searches. A cache of the caller's
  `TextureCatalog` inside `MeshCatalog` would remove the argument and would also
  pin a catalog to one archive for its whole life; neither is obviously right, so
  the argument stayed.
* **`HeaderDisagreement` has never fired on real or synthetic data.** Both readers
  read the same 40 bytes from the same slice, so it is a guard against a future
  wiring change, not an observed failure mode. It is not covered by a test that
  can make it happen, and it is recorded as such rather than left implied.

## Test inventory

All in `crates/cs_content/src/mesh.rs`, all `accept_f10_c_03_`:

| Test | What fails if the behaviour is removed |
| --- | --- |
| `..._uv_seam_survives_the_container_to_upload_boundary` | AC03 end to end. Without any production step of the path the payload has no mesh, or the seam is welded, or a row loses its contract field. Also the only test that proves the F10-C.02 audit is reached **through** a container and lands on the mesh's own row and payload. |
| `..._truncated_container_is_a_failed_row_and_recovers_after_remount` | Removes the failed-container row, the reader's offset, the stale-session refusals, the retry, or the remount recovery, and it fails. |
| `..._a_refused_mesh_is_a_row_and_nothing_is_uploaded_for_it` | Removes the per-mesh refusal (the mesh would be dropped or uploaded), or either reader-context path. |
| `..._a_payload_owns_its_data_and_survives_its_session` | Removes the payload's ownership (it would not compile/borrow correctly or would not answer), the catalog serial, or the stale-session refusal. |
| `..._a_container_of_another_family_is_never_read_as_gamez` | Removes either family refusal. |
| `..._retail_world_meshes_reach_the_upload_payload` (`#[ignore]`) | The whole path on the original installation. |

### Sensitivity probes actually run

The claims above were checked, not asserted. Each probe is a one-line mutation
of the production code, run against `cargo test -p cs_content --lib -- accept_f10_c_03_`,
then reverted:

| Mutation | Tests that failed |
| --- | --- |
| `VertexKey` built from `position` only (a position-keyed splitter that welds the seam away) | the AC03 end-to-end test, the payload-owns-its-data test, the truncated-container test |
| `if false` in place of the `family() != ZbdFamily::GameZ` check | the wrong-family test |
| `reader_context` returning `(container, None, None)` (the reader's offset and member dropped) | the truncated-container test, the refused-mesh test |
| `require_session` always `Ok`, and the serial comparison disabled (stale state served) | the payload-owns-its-data test, the truncated-container test |

The refused-mesh path and the remount recovery are covered by assertions on
values that only exist because of the production code — `failure.offset ==
Some(nodes_offset)`, `failure.member == Some("gamez.meshes.polygon…")`, and the
three rows that only appear after a remount — so removing the behaviour cannot
leave the test vacuously green.

## Sources

* [S02], [S03], [S08] in `docs/research/SOURCES.md` — the pinned reference the
  layout came from, via the F10-B and F10-C.02 findings.
* `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` — the mesh-section
  worksheet the synthetic writer encodes.
* `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` — the
  material-section worksheet, the texture-name encoding, and the measured
  `Sky1.tif` / `sky1` naming difference that keeps rows blocked.
* `docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md` — the
  session, catalog, retry and upload-boundary shape this mirrors.
* `docs/contracts/IDENTITY-CONTENT.md` — the catalog element fields and the
  "collections cannot exclude failed entries" rule.

No code was copied from any reference; mech3ax is EUPL-1.2 and is read as a
reference only. No original game data is committed, and the retail test reads
`$CS_GAME_DIR` only.
