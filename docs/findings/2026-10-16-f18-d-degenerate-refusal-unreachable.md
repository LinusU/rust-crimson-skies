# F18-D's no-extent capture refusal is unreachable through `RenderMesh::build` (2026-10-16)

Task #1449 (`F10-D1-GPU-CAPTURE-TESTS`); owner decision on the task, 2026-10-15.
Earlier analysis: #1464 / #744.

## What changed underneath the tests

F10-D.1 (#395) made `outline_draw` the original's measured per-face test: an
outline whose first three stored corners have a zero normal is
`OutlineDraw::Culled` and contributes no draw triangle (static evidence,
`0x554bf7`–`0x554ce9`; not `verified_original`). That rule stays.

Two consequences for `capture_world_mesh`, both measured with
`cargo test -p cs_app --test world -- accept_f18_d --include-ignored`:

* A mesh with no extent can only hold zero-normal faces, which the cull
  removes. `one_point_mesh()` therefore builds to no triangles and is refused
  as `EmptyMesh { groups: 0 }` in `upload_all`, before the bounds are measured.
  `GpuCaptureError::DegenerateBounds` is **unreachable through
  `RenderMesh::build`**. It is kept as defence in depth, with a doc line saying so.
* The old uniform-frame fixture (`0, 0, 1` over two positions) is a zero-normal
  face too, so it was culled to `EmptyMesh` as well. A non-culled face always
  has a real first fan triangle, so "uploads but every stored step is
  degenerate" cannot be built any more. The `UniformFrame` gate is now pinned
  by a triangle seen exactly edge-on (its plane contains the capture view
  direction and the bounds centre): non-zero normal, kept by the cull, extent
  on every axis, zero projected area.

## What the tests pin now

`accept_f18_d_a_capture_that_drew_nothing_is_refused_rather_than_written`
keeps its property: a capture that drew nothing is refused by name and leaves
no file. The `empty_mesh()` assertion is unchanged; the one-position fixture
expects `EmptyMesh` (reason: the F10-D.1 cull); the uniform-frame case uses the
edge-on triangle. Nothing accepts "either error". `crates/cs_app/tests/camera/coverage.rs`
asserts `EmptyMesh` on a mesh with *no polygons*, which the cull does not
affect, and needed no change.

## Not part of this task

The `evidence_report_*` targets and `accept_f18_d_a_gpu_capture_proves_...`
need `CS_EVIDENCE_DIR` / a provisioned `private/evidence/` and are harness
preconditions. Regenerating `docs/findings/evidence/F18-D.json` goes through its
documented harness.
