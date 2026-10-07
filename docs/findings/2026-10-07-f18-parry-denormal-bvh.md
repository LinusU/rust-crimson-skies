# #656: `c3`'s colliders build — a declared canonicalisation at the upload boundary

Date: 2026-10-07. Author: swe2-max-1. Task: Rally #656
(`F18-PARRY-DENORMAL-BVH`), the follow-up #639 filed.
Feature sheet: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
stage `### F18-B`. Parent finding:
`docs/findings/2026-10-05-f18-world-units-containers.md`.
Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and
ordinary build/test. `gpu` and `audio` were available and **not used**: nothing
is rendered or played and no original run happened, so nothing here is
`verified_original`.

## Files

- `crates/cs_app/src/render/bevy_mesh.rs` (an F17-B owner path this task was
  handed): the declared rule `SUBNORMAL_POSITION_CLAIM`
  (`f17-b.subnormal-position-flushes-to-zero`), the
  `canonicalise_position_component` it names, and the per-group
  `GroupReport::subnormal_components` count that makes the rule's footprint
  measurable instead of invisible.
- `crates/cs_app/src/world/meshes.rs` (an F18 owner path): the count carried
  through `WorldMeshGroup` and summed by `WorldMesh::subnormal_components`.
- `crates/cs_app/tests/world/world_units.rs` (an F18 owner path): the pin test
  rewritten as `the_subnormal_blocker_is_canonicalised`, the new unignored
  synthetic regression `the_declared_flush_reaches_the_collider`, `C3`'s
  `settles: true`, `SETTLE_BLOCKERS` emptied (the constant stays, so a *new*
  blocker is still named by a failure rather than skipped), and `settles` made
  `pub(crate)` so the evidence harness measures the same way the tests do.
- `crates/cs_app/tests/world/evidence_f18_parry_denormal_bvh.rs` (new, inside
  the `tests/world/` owner path): the `retail`-capability evidence harness of
  `docs/contracts/CLI-EVIDENCE.md` — an ignored test, never part of the
  acceptance selection, that writes `acceptance.json` from the recorded suite
  log plus a second production run (`parry-denormal-census.json`).
- `crates/cs_app/tests/world/main.rs` (wiring only): `mod
  evidence_f18_parry_denormal_bvh;` and the module note.
- `docs/findings/2026-10-05-f18-world-units-containers.md` (edited): the `c3`
  row marked resolved, the pin language superseded, the limitation struck with
  a pointer here.
- `docs/findings/evidence/F18-PARRY-DENORMAL-BVH.json` (new): the validated
  report copy.
- This file.

**No reader refusal was weakened, and no test was weakened.** The pin the task
replaced (`the_settle_blocker_is_named_not_hidden`) was itself an assertion that
would fail in the "settle now finishes" direction; its replacement keeps every
measurement it carried — the stored bit patterns, the blocked-set accounting —
and adds the ones the resolution needs.

## The problem, restated

#639 measured it, and the measurement was re-derived here rather than trusted:
`ZBD/C3/gamez.zbd` mesh slot 447 stores two position `y` components as
subnormal `f32`s — `0x00000003` (`4e-45`) and `0x80000006` (`-8e-45`) — on a
plane that is otherwise exactly `y = 0`. `read_f32` decodes the bytes
verbatim, so they reach the upload intact. Avian's `TrimeshFromMesh` derives
the collider from the uploaded buffer and calls parry 0.27's trimesh builder,
whose `rebuild_bvh` hardcodes `BvhBuildStrategy::Binned`. The binned builder
computes

```
bin = (NUM_BINS * (1 - eps) / (centroid_max - centroid_min)) * (c - centroid_min)
```

A **subnormal** centroid extent overflows that `f32` division to `inf`, the
`as usize` cast saturates to `usize::MAX`, and the 8-entry bin array is indexed
out of bounds — a panic escaping `App::update()` that left 33 of `c3`'s 374
colliders unbuilt. An extent of exactly zero does not panic (`inf * 0` is
`NaN`, which casts to `0`), which is what makes flushing — not a wider
rewrite — the narrow rule the corpus supports.

## The decision, and why the alternative is dead on this version

The task offered two paths. The Avian/parry configuration path was checked
first and is **not available**: `avian3d` 0.7 exposes
`trimesh_from_mesh_with_config` and `TriMeshFlags` control preprocessing
(duplicate merging, degenerate deletion, topology, internal edges), but the
BVH build is `rebuild_bvh`'s own call with `BvhBuildStrategy::Binned`
hardcoded — no flag avoids the binned partition or changes its arithmetic, and
changing dependency versions or vendoring is outside this task's scope.

So the resolution is the stated canonicalisation at the upload boundary:

> **`f17-b.subnormal-position-flushes-to-zero`.** A stored **position**
> component for which `f32::is_subnormal()` holds uploads as
> `0.0f32.copysign(component)` — `0x0000_0003` as `+0.0`, `0x8000_0006` as
> `-0.0` — and **every** value that is not subnormal uploads with its stored
> bit pattern unchanged. `±0.0`, `f32::MIN_POSITIVE`, NaN and infinity all pass
> through verbatim; the sign bit is never discarded. Normals, UVs and corner
> colors are not covered: no measured consumer of them needs it, so they stay
> bit-exact.

A subnormal is below `2^-126` of a stored unit — below the resolution of any
representable distance — and this is the same canonicalisation hardware
flush-to-zero performs. It is an **engine-compatibility rule of this
project**, carried on a claim id and counted (`GroupReport::subnormal_components`,
surfaced as `WorldMesh::subnormal_components()`): not a claim about what the
original engine did with these bytes, which stays **unmeasured**.

The rule sits in `build_upload`, the one adapter every stored mesh passes
through, so presentation and collision — which share the uploaded `Mesh` —
get the same canonicalised positions and cannot drift apart.

## What was measured after the change

- The stored bytes are untouched: `c3` slot 447 still carries exactly
  `{0x00000003, 0x80000006}` — the reader stays faithful, the premise is
  checked rather than assumed fixed.
- The production upload of that mesh reports exactly **2** flushed components;
  the uploaded position buffer contains no subnormal, contains both signed
  zeros of the flushed signs, and every uploaded position is bit-equal to a
  stored position or its signed-zero canonicalisation — nothing else changed.
- **All eight** world groups finish the settle, and every collider the spawn
  reports is built as a triangle mesh — **374 for `c3`**, not the 341 the panic
  left behind. `SETTLE_BLOCKERS` is empty.
- The synthetic regression reproduces the corpus mechanism without original
  bytes: three triangles authored so their leaf-AABB centers span a subnormal
  `y` extent on the corpus's own bit patterns (parry's `Bvh::from_iter`
  special-cases one and two leaves, so three is the smallest input that
  reaches the binned partition). With the rule removed the settle panics
  inside `App::update` — the failure this task was filed for; with it the
  collider builds.

## Test inventory

| `accept_f18_world_units_containers_` test | Covers | Fails when |
| --- | --- | --- |
| `the_declared_flush_reaches_the_collider` (**synthetic — runs in CI**) | the claim id exists; the authored IR keeps the two corpus bit patterns subnormal; `insert_render_mesh` reports exactly 2 flushes; the uploaded buffer holds no subnormal and both signed zeros; the harbor settle builds the hangar's collider and it is a trimesh | the canonicalisation is removed — the upload assertions fail **and** the settle panics inside `App::update` (both verified by mutation) |
| `the_subnormal_blocker_is_canonicalised` (retail) | slot 447's stored bytes still carry exactly the two subnormals; the upload's flush count is 2; every uploaded position is a stored position or its signed-zero canonicalisation; every group's settle finishes and every reported collider is a built trimesh | the rule stops running, flushes a non-subnormal, loses a sign, or any group's settle or collider build regresses |

The other four tests of the suite are #639's, unchanged.

**Sensitivity.** The canonicalisation call in `build_upload` was removed; both
new tests fail — the synthetic one at the buffer assertion and at the settle
(the panic is caught and named), the retail one at the `subnormal_components`
report, the signed-zero buffer checks and the `C3` settle. Restored, all pass.
No other mutation was needed: the report field and the buffer are the only
places the rule can hide.

## Unknowns and limitations (recorded, not guessed)

- **What the original engine did with subnormal stored positions is
  UNMEASURED.** `retail` is file access, not an original run. The rule is
  argued from the corpus — the values are below any representable distance and
  hardware FTZ is the same canonicalisation — not from observed original
  behavior. If original-run evidence ever shows the 2000 engine treating them
  differently, this rule is the single stated seam to revisit.
- **Only `y` was observed subnormal, and only in `c3`.** The rule covers any
  subnormal position component of any uploaded mesh and reports the count. The
  census records the two count domains separately: `c3`'s mesh array stores
  **38** subnormal position components — slot 447's pair plus 36 more on
  meshes whose centroid extents are normal, so they never panicked — and the
  upload canonicalised **55** emitted components, more than the stored count
  because the IR expands a stored position into a render vertex per distinct
  corner tuple and material group. The census rows record every group's
  stored count (`parry-denormal-census.json`). The footprint is measured, not
  projected; the two domains are recorded, not reconciled.
- **Normals, UVs and colors stay bit-exact** — a subnormal in those attributes
  would upload verbatim. No measured consumer needs otherwise; adding one
  silently would be the fabrication the rule exists to avoid.
- **parry's builder is not fixed** — the flush removes the one corpus input
  that reaches the panic. A different denormal-extent construction upstream of
  the upload would hit the same code; nothing here claims the dependency is
  safe, only that the production path never hands it a subnormal extent.

## Sources used

- `crates/cs_app/src/render/bevy_mesh.rs` (`upload_groups`, `build_upload`,
  `GroupReport`) — the upload boundary the rule lives at.
- `crates/cs_app/src/world/meshes.rs` (`WorldMesh`, `WorldMeshGroup`,
  `WorldMeshes::insert_render_mesh`) and `crates/cs_app/src/world/spawn.rs`
  (`spawn_world`, `SpawnedWorld`) — the production presentation/collision path.
- `crates/cs_formats/src/gamez/reader.rs` (`GameZMeshes`) and
  `crates/cs_formats/src/io.rs::read_f32` — read, not modified; the bytes are
  the store's, decoded verbatim.
- parry3d 0.27.0 `src/partitioning/bvh/bvh_binned_build.rs` (the binned
  partition arithmetic) and `src/partitioning/bvh/bvh_tree.rs::from_iter` (the
  one- and two-leaf special cases that set the reproduction's minimum size);
  `src/shape/trimesh.rs::rebuild_bvh` (the hardcoded `BvhBuildStrategy::Binned`
  that rules out the flag alternative); avian3d 0.7.0 `ColliderConstructor::
  TrimeshFromMesh` — read as the dependency it is; no code copied, no version
  changed.
- `docs/findings/2026-10-05-f18-world-units-containers.md` — the parent
  measurement this resolution rests on.

## Commands run

```sh
cargo fmt --all -- --check                                       # clean
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # clean
cargo test --workspace --locked                                  # exit 0
cargo test --workspace --locked -- accept_f18_world_units_containers_ --include-ignored
#   6 tests: 6 run and pass (5 need CS_GAME_DIR, 1 is the synthetic regression)
python3 tools/validate_evidence.py private/evidence/F18-PARRY-DENORMAL-BVH/acceptance.json \
  --artifact-root private/evidence/F18-PARRY-DENORMAL-BVH --require-pass   # valid
```

Evidence: `private/evidence/F18-PARRY-DENORMAL-BVH/acceptance.json` (committed
copy: `docs/findings/evidence/F18-PARRY-DENORMAL-BVH.json`), built by
`crates/cs_app/tests/world/evidence_f18_parry_denormal_bvh.rs` — see its module
doc for the regeneration sequence.
