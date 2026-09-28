# F10-B: validated n-gon triangulation

Date: 2026-09-29. Task: F10-B "Implement the verified CS GameZ mesh subset"
(Rally #42, `specs/F10-gamez-mesh-topology-and-material-records.md`,
section `### F10-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required). Implementer: claude-1 (Claude Opus 5.5).

## Scope of this slice

The stage's minimum scenario is AC02: *a concave polygon triangulates
without triangles outside its boundary*. That is what this slice delivers,
on the production path every GameZ mesh reader shares
(`RawMesh::topology`).

**Not in this slice: parsing `gamez.zbd` / `planes.zbd` bytes.** The
research boundary of the sheet forbids coding a binary layout that is not
established first, and the committed research pack
(`docs/research/FORMAT-NOTES.md`, "GameZ/planes and images") only says the
pinned legacy mech3ax revision exposes nodes, meshes and materials; it
documents no field layout. F06-A records the GameZ header as undocumented
and F10-A lists every mesh layout as unknown. Establishing that layout
from the pinned reference (S02, v0.6.0) and checking it against the
installation is a separate format-variant slice and is filed as a
follow-up task (see below) instead of being guessed here.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/gamez/polygon.rs` (new): `triangulate_polygon`
  and `NgonIssue` (`CoincidentCorners`, `ZeroArea`, `SelfIntersecting`,
  `NoEar`; `code`, `Display`).
- `crates/cs_formats/src/gamez/mesh.rs`: `RawMesh::topology` triangulates
  `PrimitiveKind::Polygon` outlines with more than three corners instead
  of rejecting every one; `FaceIssue::UnsupportedNgon` now carries the
  `NgonIssue` reason; `MeshTriangle::step` documents its polygon meaning.
- `crates/cs_formats/src/gamez/mod.rs`: module declaration, re-exports and
  module doc.
- `crates/cs_formats/tests/gamez/ngon.rs` (new, `mod ngon;` in
  `main.rs`): five `accept_f10_b_*` tests (a sixth added in review).
- `crates/cs_formats/tests/gamez/main.rs`: the F10-A fixture that expected
  a convex quad to be `UnsupportedNgon` now uses a symmetric bow tie,
  which is still unsupported (`ZeroArea`). The decoded/invalid/unsupported
  counts it asserts are unchanged.

**One observable failure:** a triangle fan from corner 0 of the arrow
outline `(0,0) (4,0) (4,4) (2,1) (0,4)` draws `(0, 2, 3)`, which is
clockwise and lies over the notch, outside the outline
(`accept_f10_b_concave_polygon_triangulates_inside_its_boundary` fails).

## Design decisions

- **Ear clipping after validation, never a fan** (non-negotiable #2). The
  outline is first checked: no two corners at one location (exact `f32`
  equality; a repeated position index is the same case), non-zero Newell
  normal, and a simple projected outline (non-adjacent edges never touch,
  adjacent edges never fold back). Only then are ears clipped. An outline
  failing a check is `FaceIssue::UnsupportedNgon { corners, reason }` with
  no triangles; the rest of the mesh still decodes.
- **Projection.** The outline is projected onto the coordinate plane its
  Newell normal is most aligned with, mirrored so it is counter-clockwise
  there. Ears are convex relative to the outline's own orientation, so the
  triangles keep the stored winding, clockwise or counter-clockwise, in any
  plane. Which winding is front-facing is still unknown (F10-A).
- **No tolerance constants.** All predicates compare against zero without
  a tolerance, on the stored `f32` values widened to `f64`. Coordinate
  differences and their products are exact in `f64` for ordinary ranges,
  but the final subtraction can round, so the predicates are not exact in
  general. No epsilon was invented; a numerically hopeless outline
  surfaces as `NoEar`, not as a guessed triangle.
- **Deterministic output.** The first ear in stored order is clipped, so
  the same outline always yields the same triangles. Each `MeshTriangle`
  keeps the source-corner map (`corners`) and its place in the
  triangulation (`step`).
- **Unsupported, not invalid.** Rejected outlines count in
  `MeshTopology::unsupported_faces`, separate from invalid indices and
  values, so F10-D's AC04 report can tell them apart.

## Test inventory (`accept_f10_b_*`)

All in `crates/cs_formats/tests/gamez/ngon.rs`; every one calls
`RawMesh::topology` and/or `triangulate_polygon`. Inside/outside, area and
winding are judged by helpers in the test file (shoelace area, even-odd
point-in-polygon, Newell normal), not by the code under test.

| Test | Covers |
| --- | --- |
| `concave_polygon_triangulates_inside_its_boundary` | AC02: arrow → 3 triangles, each with the outline's winding, centroid inside, total area exact; the fan triangle `(0,2,3)` absent; mesh path equals direct call |
| `many_reflex_corners_still_cover_the_outline` | 16-corner comb with four reflex notches, stored counter-clockwise and clockwise |
| `outline_off_the_xy_plane_keeps_its_stored_winding` | arrow in the tilted plane z = y and in x = 0, both directions; winding checked against the stored outline's Newell normal |
| `convex_quad_decodes_to_two_triangles` | the case F10-A rejected now decodes |
| `untriangulable_outlines_are_reported_not_fanned` | asymmetric bow tie, edge folding back, collinear corners, coincident corners, a repeated position index; no triangles, counted as unsupported, the other polygon still decodes |
| `random_simple_outlines_are_covered_exactly` | added in review: 500 deterministic pseudo-random simple outlines (4–12 corners, either winding, concave or convex), built from integer grid points untangled by 2-opt with integer predicates independent of the triangulator |

## Mutation probes

Applied to `polygon.rs`, `cargo test -p cs_formats --test gamez --
accept_f10_b_` run, file restored:

| Mutation | Failing tests |
| --- | --- |
| fan from corner 0 instead of ear clipping | 4 |
| simplicity check skipped | 1 |
| ear convexity test removed | 2 |
| ear containment test removed | 4 |
| coincident-corner check removed | 1 |

## Recorded unknowns

- **Every CS GameZ mesh layout** (unchanged from F10-A): which stored
  records hold positions, normals, polygons, corner indices, UVs, colors,
  flags and materials, and which flag selects strip vs. polygon. Filed as a
  follow-up research/parser task.
- **How the original renderer handled n-gons** (fan, strip or its own
  triangulation) and whether any outline this module rejects occurs in
  retail content. Only the private corpus can say; that count is AC04 of
  F10-D.
- **Non-planar outlines.** The projection accepts them when the projected
  outline is simple; whether retail n-gons are planar is unknown.

## Review

Reviewer: claude-1 (Claude Opus 5.5), a fresh session with no context from
the implementation session but the same agent name and model as the
implementer, so this is not an independent-model review. Checked the
slice against the F10 sheet, AGENTS.md and the owner paths. A throwaway
fuzz run (not committed) triangulated 200 000 star-shaped outlines with
collinear corners and about 11 500 random simple outlines with no
failure: every result covered the outline area exactly with triangles of
the outline's winding. The review added
`accept_f10_b_random_simple_outlines_are_covered_exactly`; it fails when
the ear containment or ear convexity test is removed. It also corrected
the wording on exactness above. The deferral of byte parsing to #363 is
accepted: the research boundary forbids guessing the layout, and the
stage's "Done when" names only AC02. F10-C (#43) needs #363's reader
before it can wire a real producer.
