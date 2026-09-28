# F10-A: lossless mesh IR and strip topology fixtures

Date: 2026-09-28. Task: F10-A "Define lossless mesh IR and topology test
fixtures" (`specs/F10-gamez-mesh-topology-and-material-records.md`, section
`### F10-A`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/gamez/strip.rs` (new): `MIN_STRIP_INDICES`,
  `StripTriangle` (`step`, `corners`, `indices`, `is_degenerate`),
  `StripError` (`TooShort`; `code`, `Display`, `Error`) and
  `decode_strip`.
- `crates/cs_formats/src/gamez/mesh.rs` (new): the IR (`RawMesh`,
  `RawPolygon`, `RawCorner`, `PrimitiveKind`) and its topology report
  (`RawMesh::topology` → `MeshTopology` with `MeshTriangle` and one
  `FaceStatus` per polygon; `FaceIssue` with `code`/`is_unsupported`).
- `crates/cs_formats/src/gamez/mod.rs` (new): module doc and re-exports.
- `crates/cs_formats/src/lib.rs` (wiring only): `pub mod gamez;` and one
  module-doc paragraph.
- `crates/cs_formats/tests/gamez/main.rs` (new; integration-test target
  `gamez`): the unit-square fixture and six `accept_f10_a_*` tests.
- This file.

**Not created in this stage:** `crates/cs_content/src/mesh.rs` (render
vertex splitting and catalog wiring, AC03, is F10-C) and
`crates/cs_app/src/mesh.rs` (upload, F10-C). Nothing to connect until
F10-B parses a real mesh variant; same reasoning F08-A recorded.

**One observable failure:** a strip decoder that does not swap the first
two corners of odd steps draws `(1, 2, 3)` instead of `(2, 1, 3)` for strip
`[0, 1, 2, 3]`, and that triangle has the opposite signed area
(`accept_f10_a_strip_0123_keeps_one_orientation` fails). A decoder that
removes the repeated index of `[0, 1, 2, 1, 3]` before counting parity
draws a different square (`accept_f10_a_degenerate_insertion_still_
advances_parity` fails).

## Design decisions

- **Strip parity is positional.** Step `k` uses strip positions
  `k, k+1, k+2`, odd steps swap the first two. Degenerate steps are
  returned (marked by `is_degenerate`), never removed, so parity is counted
  from the strip start. `decode_strip` returns exactly `len - 2` steps.
- **Winding is relative.** Every triangle of a strip has the winding of
  its first triangle as stored. Which winding is front-facing in CS content
  is **not** decided here (see unknowns).
- **Degenerate means equal indices.** Two distinct indices at the same
  position are not treated as degenerate; that would be a geometric
  judgement the reader has not earned.
- **Nothing is dropped.** `RawMesh::topology` returns one `FaceStatus` per
  stored polygon. A rejected polygon names its issue (too few corners,
  position/normal index out of range, non-finite position/normal/UV/color,
  or an n-gon this stage cannot triangulate) and the rest of the mesh is
  still decoded. `invalid_faces`, `unsupported_faces` and `decoded_faces`
  are the exact counts AC04 will report per private asset in F10-D;
  `is_complete` is the gate before upload (non-negotiable #4).
- **N-gons are unsupported, not fanned** (non-negotiable #2). A
  `PrimitiveKind::Polygon` with more than three corners is
  `FaceIssue::UnsupportedNgon`, kept separate from invalid faces. Validated
  triangulation is F10-B (AC02).
- **Per-corner attributes, raw fields.** UV, color and normal index belong
  to the corner, not the position; `MeshTriangle::corners` is the
  source-corner map back to them (non-negotiable #3 groundwork). Polygon
  flags and material index are stored as raw `u32` with no bit or field
  interpreted (non-negotiable #5: nothing named "specular" or "soil").
- **Fixtures are authored in the test file**; no binaries committed.
  Orientation is checked by a signed-area helper written in the test,
  independent of the decoder.

## Test inventory (`accept_f10_a_*`)

All in `crates/cs_formats/tests/gamez/main.rs`; every one calls
`decode_strip` and/or `RawMesh::topology`.

| Test | Covers |
| --- | --- |
| `strip_0123_keeps_one_orientation` | AC01: `[0,1,2,3]` → `(0,1,2)`, `(2,1,3)`, both counter-clockwise |
| `degenerate_insertion_still_advances_parity` | AC01: `[0,1,2,1,3]` draws the same square, degenerate step kept and counted |
| `parity_counts_from_the_strip_start_not_the_first_drawn_step` | a leading duplicate flips the whole strip consistently; compaction would not |
| `short_strip_is_rejected_not_dropped` | 0/1/2 indices rejected; mesh keeps decoding other faces |
| `every_broken_or_unsupported_face_is_counted` | each `FaceIssue`, exact decoded/invalid/unsupported counts |
| `shared_position_keeps_per_corner_attributes` | two UVs on one position both reachable through the corner map; raw flags/material unchanged |

## Mutation probes

Applied to production code, `cargo test -p cs_formats --test gamez` run,
file restored:

| Mutation | Failing tests |
| --- | --- |
| odd steps not swapped | 3 |
| repeated adjacent indices removed before decoding | 1 |
| n-gons decoded as strips instead of reported | 1 |
| corner bounds/finiteness not checked | 1 |

## Recorded unknowns

- **Every CS GameZ mesh layout.** Which stored records hold positions,
  normals, polygons, corner indices, UVs, colors, flags and materials in
  `gamez.zbd` / `planes.zbd` is not established in the committed research
  pack; F06-A records the GameZ header as undocumented. F10-B must read the
  pinned legacy CS-capable reference (S02/S06) and check the installation.
- **Which flag selects strip vs. polygon.** Unknown; the reader supplies
  `PrimitiveKind` once it is established.
- **Front-face winding and handedness.** Unknown for CS content.
- **Material record layout** and the meaning of every polygon flag bit.
  Unknown; kept raw.
- **Corner color representation.** The IR carries `[f32; 3]` as a designed
  canonical form; how stored colors map onto it is a variant fact.
