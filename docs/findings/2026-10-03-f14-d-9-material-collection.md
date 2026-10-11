# F14-D.9: the material collection of the baseline inventory

Date: 2026-10-11. Task: #558 "Address the material records of a GameZ container".
Feature sheet: `specs/F14-canonical-content-catalog-and-dependency-closure.md`
(stage `### F14-D`); reader sheet `specs/F10-gamez-mesh-topology-and-material-records.md`.
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities used:
**`retail`** (read-only `$CS_GAME_DIR`) and ordinary build/test. `gpu` and `audio`
were not used; the original game was not run, so nothing here claims
`verified_original`.

## The gap and the change

F14-D.4 (#487) populated `scene_node` and `mesh` but no `material` row, because
`cs_formats::gamez::RawMaterial` carried no byte offset and the baseline would
have had to repeat the reader's layout arithmetic. Now:

- `cs_formats::gamez::RawMaterial` has `data_offset` and `data_end`: the absolute
  container offsets of the 44-byte slot (40-byte record + two link words), taken
  from the reader's own position while it reads the slot. The cycle frame data
  stored after the 1000-slot array is not inside a slot's extent. (Wiring: the
  struct literals of `cs_content/src/mesh.rs` and `cs_app/tests/render/matrix_wide.rs`
  fixtures gained the two fields, set to 0.)
- `cs_content::catalog::baseline::retail_baseline` holds one `material` row per
  **present** record of each of the nine containers. Identity:
  `<container key>.<material slot>`, the shape the `mesh` rows use. Span: the
  reader's extent. Zero stub slots have no row.
- Edges (all static): onto the container's `install_file` row, and onto the `mesh`
  row of each mesh that references the slot. Both places a material index is stored
  count: a mesh's 12-byte material references and every polygon's material
  groups. A referencing mesh with no `mesh` row (no node names its slot) gives no
  edge rather than one onto an id nothing holds.
- A textured material whose texture index (or a cycle frame's) is outside the
  container's texture-name table carries an explicit
  `f14.d.9.baseline.material_texture_unresolved` unknown.
- Nothing is normalized (the stored colour is a raw `f32` triple), no row claims
  a consumer, every row is unavailable, and the denominator does not move.
- The section is read independently of nodes and meshes: a refused material
  section costs that container its material rows and is named in the material
  collection's diagnostic; its nodes and meshes remain.
- The collection status counts `texture_index_without_texture_name` and
  `referenced_slot_without_material`; `geometry_containers` in the report gained
  `materials`, `material_textures_unresolved` and
  `material_references_without_record` (the F14-D.4 pinned JSON string was
  extended with them, nothing weakened).

## Measured on this installation

Nine containers, 9 x 1000 slots, **4 669 present records** (planes 954, C1 570,
C1B 320, C1C 284, C2 492, C2B 274, C3 483, C4 685, C5 607), equal to the F10-C.02
corpus counts. Of the 9 000 slots, 4 331 are zero stubs and have no row.

- Texture indices outside the texture-name table: **0**.
- Mesh-referenced material slots with no present record: **0**.
- Mesh edges: **29 904**; materials with no mesh edge: **191** (referenced by no
  mesh, or only by a mesh no node names).
- Unreadable material sections: 0. Coverage: 84 278 unreachable rows (was 79 609),
  `ready` 0, `unresolved_references` 0, roots 53 — unchanged.

## Limits (also in the evidence report)

Stored colour and the field newer work calls soil are raw (F10/F16). The texture
a material names is not a catalog edge because the baseline has no `image` row.
Edge direction is material onto mesh, as the task states. Cycle frame data is not
inside a row span. The polygon-material-group reference path is exercised by the
retail run only (the synthetic fixture references through the mesh's material
list).

## Mutation probes (implementer)

Shortening `data_end` by 4 in the reader: killed by one reader test and one
baseline test. Dropping the material rows from `retail_baseline`: killed by all
five synthetic baseline tests. Gaps and the report fields are pinned by the
status and report tests.

## Evidence

`docs/findings/evidence/F14-D.9.json`, produced by
`crates/cs_content/tests/evidence_report_f14_d_9.rs`, validated with
`tools/validate_evidence.py --require-pass`. Implementer: sonnet-1; no
independent review yet.
