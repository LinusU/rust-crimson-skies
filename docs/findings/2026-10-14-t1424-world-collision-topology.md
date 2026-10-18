# T1424: world colliders are built from the canonical topology, not the drawn fan

**Task:** #1424 — *World colliders should not be built from the drawn (fan)
topology* (found during F10-D.1 #395, recorded in
`docs/findings/2026-10-14-f10-d-1-what-the-original-renderer-drew.md`, honest
limits).
**Capabilities used:** ordinary build/test only. The retail corpus counts
cited below are F10-D.1's measurement as recorded in
`docs/findings/2026-10-14-f10-d-1-what-the-original-renderer-drew.md`; this
task reuses them as recorded evidence and does not re-measure them. No original
executable was run, no GPU render, no audio, no human play.
**Claim class:** `implemented`. No claim about original physics behaviour:
nothing here is `verified_original`.
**Contract:** `docs/contracts/IDENTITY-CONTENT.md`.

## The defect, restated

`ColliderConstructor::TrimeshFromMesh` derives a collider from the `Mesh3d`
on the entity, and that `Mesh3d` holds the **draw** buffer — the original's
measured `D3DPT_TRIANGLEFAN` outline fanned from corner 0
(`OWNER-STATIC-2026-10-05`). Rendering and collision shared one index buffer.

Sharing it costs more than tidiness. A fan triangle that folds against its own
outline is a leaf all three of whose corners parry's
`Triangle::local_support_point` can tie on (it returns `c`);
`closest_points_cuboid_triangle` trusts that tied pick as the closest vertex,
so `compute_toi`'s bisection lands mid-window and `SweptCcd::NON_LINEAR`
clamps half a tick of travel with no contact. F10-D.1 measured this on the
hangar-shell fixture sweep: the `(0, i, i+1)` emission order phantom-hits the
right leg's `+x` leaf, `(i, i+1, 0)` the mirrored left-leg leaf, `(i+1, 0, i)`
none of the measured paths. That choice was empirical and fixture-bound — any
leaf set can trip the artefact under any rotation, which is why the fix is to
stop sharing the buffer rather than to keep rotating triples.

## The fix: one stored mesh, two views, two consumers

`cs_formats` already documents `RawMesh::topology()` as the collision view
(ear-clipped, validated) next to `RawMesh::draw_topology()` (the measured
fan). Task #1424 makes a consumer of each:

| stage | what it now carries |
| --- | --- |
| `cs_content::mesh::CollisionMesh` (new) | the canonical topology's index triples over the stored positions they name, compacted in first-reach order; built inside `RenderMesh::resolve` from the same `RawMesh` the draw view was built from |
| `cs_content::mesh::RenderMesh::collision()` | that view, beside `vertices()`/`triangles()` (draw) and `groups()` |
| `cs_app::render::bevy_mesh::ColliderMesh` (new) | the same pair with `SUBNORMAL_POSITION_CLAIM` applied — the one canonicalisation the physics consumer needs — and `ColliderMesh::new` for a caller that authors geometry instead of reading it |
| `GroupUpload::collision()`, `WorldMesh::collision()` | the mesh-level view travelling with the upload, so a consumer handed only group uploads can still build a collider |
| `crate::world::spawn::MeshReference` | the report record now carries **both** counts — `triangles` (drawn) and `collision_triangles` (collision) — so a consumer can tell the two views apart instead of reading one as the other |
| `crate::world::spawn::mesh_collider_constructor` | `ColliderConstructor::TrimeshWithConfig { …, flags: MERGE_DUPLICATE_VERTICES }` |

The flag is the one `TrimeshFromMesh` itself passes, so the derived shape
differs from the old path **in its index buffer and in nothing else**: same
stored positions, same duplicate-vertex merge, no triangle deleted, same BVH
build.

The two spawn paths a body is actually stopped by were changed:

* `crate::world::spawn::spawn_mesh_body` and `spawn_mesh_trigger_volume`
  (F18-B world import, solid and trigger);
* `crate::asset_stack::spawn_static_mesh_collider_on_body` — its signature
  gained the `ColliderMesh` argument, because a helper that only receives a
  `Mesh` cannot know which index buffer is the collision one.

Rendering is untouched: the entity keeps its `Mesh3d` handle, and
`shared_mesh`/`WorldMeshAssets` still put one asset behind every record that
names the same mesh.

## What was deliberately *not* changed

* **`asset_stack::spawn_static_mesh_collider`** (body + child-node layout)
  still derives from the drawn `Mesh3d`. `ColliderConstructorHierarchy` builds
  one constructor per descendant from that descendant's own handle and has no
  per-descendant explicit-triangle form. It is not world geometry: its body is
  `SweptInvisible`, a swept body passes through it at any speed above the
  discrete sampling rate, and F18-B does not use it. The limitation is stated
  on the function, and a swept-eligible hierarchy body would be its own task.
* **`RawMesh::draw_topology()` and the render gate.** F10-D.1's decision —
  the fan is what the original submitted — stands; this task only stops the
  collider from reading it.
* **Upstream parry.** Option (b) in the task description is outside this
  repository. The tie-break in `local_support_point` and the trusting of that
  pick in `closest_points_cuboid_triangle` are still there; the fix makes the
  fan's fold the only way a stored mesh can reach them.

## The disagreement the two views can have

The views are allowed to disagree, and one disagreement is a **panic**:
`Collider::trimesh` / `TrimeshWithConfig` panic on an empty index buffer where
`TrimeshFromMesh` returned `None` and logged. A mesh the render gate accepts
(what the original would fan) can be one `RawMesh::topology()` refuses — a
self-intersecting outline comes out `Rejected(UnsupportedNgon { reason:
ZeroArea })` while the fan decodes it into two triangles.

`spawn_object` therefore reports that object instead of spawning it:

> `SkipReason::CollisionTopologyUnavailable` — presented, no collider, one
> visible skip entry naming the reason.

No geometry is invented in either direction: it is neither silently solid nor
silently removed. Measured on the retail runs below: every record of the
playtest retail area still collides, so no retail mesh there landed on this
skip. Whether *some* retail mesh could is not asserted — the corpus is walked
by the retail acceptance tests, and a mesh that did land on it would show up
there as a new skip rather than as a crash.

## Tests

`crates/cs_app/tests/accept_t1424_collision_topology.rs`, both production-path
(`RenderMesh` → `upload_groups` → `WorldMeshes` → `spawn_world` → Avian):

* `accept_t1424_a_world_collider_is_the_canonical_topology_and_the_drawn_asset_is_still_the_fan`
  — a dart outline whose corner-0 fan folds over the notch the outline leaves
  open, so the two views are different *triangle sets* on the same four
  corners. Asserts, with every expected set computed from the stored
  `RawMesh`'s own two views rather than from the code under test:
  `RenderMesh::collision()` **is** `RawMesh::topology()`; the render mesh
  **is** `draw_topology()`; the derived collider **is** the canonical set and
  **is not** the fan; and the presented asset still carries the fan. Verified
  to fail when `mesh_collider_constructor` is reverted to `TrimeshFromMesh`.
* `accept_t1424_a_mesh_whose_collision_view_is_empty_is_reported_and_never_spawned_solid`
  — the bowtie above: two drawn triangles, zero collision triangles, one skip
  entry with `CollisionTopologyUnavailable`, no collider entity, presentation
  intact.

Run with:

```sh
cargo test --workspace --locked -- accept_t1424_ --include-ignored
```

Measured on this branch (`CS_CAPABILITIES=retail,gpu,audio`, `CS_GAME_DIR`
set). The four required checks were re-run **after** the final rebase onto
`0eba62ed` and are green; the retail runs are from the pre-rebase tree, which
is the same tree for every file this task touches (the rebase brought in
commits that touch none of them and no `Cargo.toml`/`Cargo.lock`):

| command | result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | clean |
| `cargo test --workspace --locked` | green |
| `cargo test --workspace --locked -- accept_t1424_ --include-ignored` | 2 discovered, 2 passed |
| `cargo test -p cs_app --test world -- --include-ignored` | 154 passed, 4 failed — all four pre-existing and unrelated (see below) |
| `cargo test -p cs_app --test playtest_retail -- --include-ignored` | 37 passed, 1 failed — pre-existing |
| `cargo test -p cs_app --test render -- --include-ignored` | 97 passed, 3 failed — all pre-existing |

The retail failures are **not** this task's, and each is named rather than
counted:

* six `evidence_report_*` / `writes_the_acceptance_report` targets panic with
  `CS_EVIDENCE_DIR is not set: this harness only runs through the sequence in
  its module doc` — an evidence-harness precondition, unrelated to colliders;
* `audit::accept_f18_d_a_gpu_capture_proves_the_stored_geometry_was_drawn`
  fails with `Io { … "private/evidence/F18-D/gpu-capture-arch.png", reason:
  "No such file or directory" }` — the private evidence directory is not
  provisioned here;
* `audit::accept_f18_d_a_capture_that_drew_nothing_is_refused_rather_than_written`
  expects `DegenerateBounds` for a three-corner outline on one stored position
  and now gets `EmptyMesh` — F10-D.1's zero-normal cull
  (`crates/cs_formats/src/gamez/polygon.rs`, landed 2026-10-14, after that
  test's last edit on 2026-10-10) makes the render gate produce no draw
  triangle for it at all, so the refusal happens one step earlier. The draw
  view is byte-identical before and after *this* task, which
  `accept_t1424_a_world_collider_is_the_canonical_topology_and_the_drawn_asset_is_still_the_fan`
  asserts directly. Filed as **#1449**.

### The retail measurement the new skip can report

The `CollisionTopologyUnavailable` guard is the one place this task can make a
retail object *stop* colliding, so it was measured rather than assumed — the
table above is that measurement.

`accept_playtest_retail_retail_c1c_area_and_bloodhawk_mesh_spawn_and_capture`
is the one that noticed the two views really diverge on retail data: record
`playtest.node-2595` stores **4** drawn triangles and **2** collision ones — a
polygon drawn once per material group — and it passed only after the assertion
was re-expressed against the right view (see below). Every record in that area
still collides, so no retail object in it landed on the new skip.

The assertion it had pinned — "collides from exactly the triangles it draws" —
is the invariant task #1424 deliberately changes, so it was **re-expressed, not
weakened**: `cs_app::world::MeshReference` now reports **both** counts
(`triangles`, `collision_triangles`), and the playtest asserts the derived
collider carries exactly `collision_triangles` — still an exact count a hull,
box or decimation would fail, and still one a regression back to the drawn fan
would fail on every record whose two views differ, node-2595 among them.

## Honest limits

* The phantom-TOI itself is **not** re-measured here. Its measurement belongs
  to F10-D.1 and `accept_f18_b_*`; what this task changes is which index
  buffer reaches parry, which is observable directly (the tests above). A
  regression that reintroduced the fan would fail those tests even if the
  hangar sweep happened not to trip the artefact on that fixture.
* Which *particular* retail leaves used to phantom-hit, and whether any still
  could through a non-fan path, is unmeasured: the tie-break is a property of
  parry, not of the fan, and option (b) was not taken.
* The collider's `TriMesh` is now **per entity** rather than shared through
  Avian's `ColliderCache` (which is keyed by mesh handle and does not apply to
  an explicit `Trimesh*` constructor). Geometry is still uploaded once — the
  `Mesh3d` handle is shared exactly as before — but each spawned object owns
  its own derived shape. Filed as **#1430** rather than fixed here, because a
  shared `Collider` would change the constructor→collider timing every
  collider-on-body test is written against. **#1430 has since landed**
  (`docs/findings/2026-10-18-t1430-shared-derived-trimesh.md`): the shape is
  derived once per upload into `WorldMeshAssets` and cloned per colliding
  object, and the collider-on-body suites were re-run unchanged against the
  directly-inserted `Collider`.
