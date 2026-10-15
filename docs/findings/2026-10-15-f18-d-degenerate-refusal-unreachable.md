# F18-D's no-extent capture refusal is unreachable on main (2026-10-15)

Found while regenerating F18-D's acceptance evidence for Rally #744
(F17-E-FLAT-CAPTURE-MSAA). Resolving task: **#1464**
(`F18-D-DEGENERATE-REFUSAL`). Affected content: the F18-D acceptance suite
(`accept_f18_d_*`, one test red), the committed report
`docs/findings/evidence/F18-D.json` (stale: its PNG digests predate both the
measured-topology build and #744's one-sample capture camera, and it cannot be
regenerated while the suite fails — its harness refuses a run whose exit code
is not 0), and every fidelity claim that leans on F18-D's GPU captures until
that report is regenerated. This gates nothing else: F17-D and F21-D evidence
were regenerated under #744 on 2026-10-15 and validate.

## The failure

```sh
cargo test -p cs_app --test world \
  audit::accept_f18_d_a_capture_that_drew_nothing_is_refused_rather_than_written \
  -- --exact --include-ignored
```

Panics at `crates/cs_app/tests/world/audit.rs:921`:

```
a mesh with no extent is refused by name, got Err(EmptyMesh { group: "fixture", mesh_index: 9, groups: 0 })
```

Expected `GpuCaptureError::DegenerateBounds`. The full suite
`cargo test --workspace --locked -- accept_f18_d_ --include-ignored` exits 101
(7 of 8 tests pass; the other seven, including both retail/GPU tests, pass).

## It is not #744's change

The identical failure reproduces with `crates/cs_app/src/world/gpu_capture.rs`
restored verbatim to `origin/main`'s version
(`git checkout origin/main -- crates/cs_app/src/world/gpu_capture.rs`) — that
is, with #744's camera commit removed. #744 changes only the camera bundle of
`spawn_scene`; the failing assertion happens in `upload_all`, before any app
or camera exists. Main's tip at the time of measurement: `46796cb2`, branch
base `7d624f5c`.

## Why

`one_point_mesh()` (`crates/cs_app/tests/world/audit.rs:972`) is a three-corner
outline on one stored position with no normals, built through the production
`RenderMesh::build`. Under `RawMesh::draw_topology` (introduced as the build
path by `6bf8cfc9` "Build render meshes from the measured draw topology",
2026-10-14, plus `c9f2f8f2` / `7bb5e66f`), a face whose first three corners
have a zero normal is **culled per face**: it contributes no triangles and the
corners are never resolved, so the built mesh has no triangles, no vertices and
no groups. `capture_world_mesh` reports `EmptyMesh` (the upload runs first)
and `DegenerateBounds` becomes unreachable for it.

The neighbouring fixture `empty_mesh()` (no polygons at all) builds to the same
observable state, so reordering the checks does not restore the test either:
bounds-first makes `stored_bounds` clamp an empty vertex list to a zero extent
and both fixtures report `DegenerateBounds`, failing the *first* assertion
(`audit.rs:896`, expects `EmptyMesh`). Both fixtures have collapsed onto one
production state; no ordering inside `gpu_capture.rs` can satisfy the test as
written.

More generally, a mesh with zero extent can only hold zero-normal faces, which
the measured topology culls — so `GpuCaptureError::DegenerateBounds` appears
unreachable for any mesh built through `RenderMesh::build`, not just for this
fixture.

The last green F18-D evidence run predates the topology work:
`6088114d` (2026-09-30).

## The decision needed (#1464)

1. **Keep the contract reachable**: restore a production path where "no
   extent" outranks "no triangles" (e.g. bounds-check from stored corners the
   capture can still see, or a fixture whose face is not culled while its
   extent stays zero), leaving the F18-D assertions untouched; **or**
2. **Owner-authorized spec decision** that the measured-topology reading
   supersedes this refusal for culled single-position outlines, with the
   expectation in `crates/cs_app/tests/world/audit.rs` updated by an
   owner-authorized change — never by an agent weakening a pinned acceptance
   test to get green.

After the suite is green, regenerate `docs/findings/evidence/F18-D.json`
through the documented harness in `crates/cs_app/tests/world/audit/evidence.rs`
(suite `tee` → `evidence_report_f18_d` → `tools/validate_evidence.py
--require-pass`) and commit the copy.
