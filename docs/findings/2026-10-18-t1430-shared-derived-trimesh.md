# T1430: one derived collision shape per mesh, not one per object

**Task:** #1430 — *Share the derived TriMesh across objects that name one mesh*
(follow-up of #1424).
**Capabilities used:** ordinary build/test, plus `retail` and `gpu` for the two
retail re-runs named below. No original executable was run and no
`human_play`/`human_review` evidence was used; the retail runs re-drive the
already-recorded production paths on the owner's installation.
**Claim class:** `implemented`. No claim about original engine behaviour;
nothing here is `verified_original`.
**Contract:** `docs/contracts/IDENTITY-CONTENT.md`.
**Supersedes the honest limit** at the end of
`docs/findings/2026-10-14-t1424-world-collision-topology.md`.

## The regression #1424 left

`ColliderConstructor::TrimeshFromMesh` resolves through Avian's `ColliderCache`
(`avian3d-0.7.0/src/collision/collider/cache.rs`), keyed by mesh handle and
constructor: N entities naming one mesh share one derived `TriMesh` behind an
`Arc`. #1424 replaced it with an explicit
`ColliderConstructor::TrimeshWithConfig { …, flags: MERGE_DUPLICATE_VERTICES }`
carrying the mesh's own **collision** view, because `TrimeshFromMesh` reads the
drawn buffer — the original's measured `D3DPT_TRIANGLEFAN` fan, whose folded
leaves trip parry's support-point tie-break into a phantom time of impact
(`docs/findings/2026-10-14-f10-d-1-what-the-original-renderer-drew.md`).

`init_collider_constructors` consults `ColliderCache` **only** when the
constructor `requires_mesh()`. `TrimeshWithConfig` carries its own buffers, so
it returns `false`, the cache is never reached, and every spawned object built
its own `TriMesh`: its own vertex and index copies and its own BVH. The *upload*
was still shared (one `Handle<Mesh>`, one `Assets<Mesh>` entry per mesh); only
the derived shape was duplicated. Measured on the twin harbor world before this
change: four records over one shell mesh, four independent shapes.

## The fix

`WorldMeshAssets` — the resource that already makes one `Handle<Mesh>` per
named mesh — now also holds the collision shape derived from that upload once:

* `SharedWorldMesh` gains `collider: Option<Collider>`;
* `WorldMeshAssets::collider(id, fingerprint)` returns a clone for the matching
  upload, and `insert_collider` records the shape once it is built;
* `WorldMeshAssets::insert` resets the shape when the reference is re-uploaded
  under a new fingerprint, so a replaced upload is never collided with through
  the previous geometry's shape;
* `spawn_mesh_collider` takes the shape from `shared_collider` (the shape
  analogue of `shared_mesh`) and hands the **owned clone** to
  `spawn_mesh_body` / `spawn_mesh_trigger_volume`, which insert it as the
  entity's `Collider` directly.

`Collider::shape()` is `SharedShape`, a `parry3d` `Arc<dyn Shape>`; cloning a
`Collider` clones that arc, so objects naming one mesh hold one allocation.
`mesh_collider` (a `Collider`) and `mesh_collider_constructor` (a
`ColliderConstructor`, still used by
`crate::asset_stack::spawn_static_mesh_collider_on_body`) both build their
buffers through one `mesh_collider_buffers`, so they cannot disagree about
geometry or flags.

### The `constructor → collider` timing

This is the reason #1424 filed #1430 instead of fixing it in place. The old
path left the `Collider` to `init_collider_constructors` (an `Update` system),
so a `Collider` existed only after the first update; the fix inserts it at
spawn. What the collider-on-body tests actually pin is **the collider is on the
rigid body's own entity**, which is unchanged — and `SweptCcdBodyQuery` reads
`&Collider` off the body, which this still satisfies. Measured, not assumed: the
named suites run unchanged (below), including the 400 m/s swept probe against
world geometry, the trigger/solid layout comparison, #1424's canonical-topology
test, and the retail world import.

Nothing is gained by keeping the constructor: an explicit `Trimesh*`
constructor is exactly the thing that *prevents* the shape from being shared,
and Avian's cache cannot be reached for it. Inserting the shared `Collider`
directly is the mechanism that makes the shape one allocation, and the entity's
own `Transform` scale is applied to its own `Collider` by Avian
(`update_collider_scale`), so the shared unscaled shape carries no pose.

### Copy-on-write on scale

`Collider::set_scale` writes `self.scaled_shape` (a new `SharedShape` for a
non-unit scale) and never mutates `self.shape`. Two objects naming one mesh
share the unscaled `Arc`; each gets its own scaled shape. This is **verified**
by `accept_t1430_a_per_entity_scale_does_not_leak_into_a_shared_shape`, which
spawns two records on one mesh at two different non-uniform scales and checks
the shared unscaled corners stay the stored geometry bit-for-bit while each
`shape_scaled()` is its own scale.

## Tests

`crates/cs_app/tests/accept_t1430_shared_trimesh.rs` (module of
`tests/suite_a.rs`), production path only (`spawn_world` over the synthetic
`twin_harbor_world` / a test-authored `RawMesh`), `Origin::SyntheticFixture`,
no original data:

* `accept_t1430_two_records_naming_one_mesh_share_one_derived_shape` — two
  solids and the body-less trigger volume naming one shell share **one** shape
  (`Arc::ptr_eq` on `Collider::shape()`, equal and `> 2` `Arc::strong_count`),
  a record on another mesh shares nothing, every shape still carries the
  collision view's 36 triangles, and the two solids keep their own translations.
* `accept_t1430_a_per_entity_scale_does_not_leak_into_a_shared_shape` — two
  records on one mesh at two different non-uniform scales: one unscaled shape,
  two different scaled shapes, each the stored corners times its own scale.

Both were **verified to fail** when the sharing is removed (a temporary
`shared_collider` that derived a shape per call): the twin test failed on
`Arc::ptr_eq`, the scale test on the shared-unscaled-shape assertion.

## Checks on this branch

| command | result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | clean |
| `cargo test --locked -p cs_app --test cs_app_tests_a` | 903 passed, 0 failed (225 ignored) |
| `cargo test --locked -p cs_app --test cs_app_tests_b` | 282 passed, 0 failed (63 ignored) |
| `cargo test --locked -p cs_app --test cs_app_tests_c` | 529 passed, 0 failed (64 ignored) |
| `cargo test --locked -p cs_app --test cs_app_tests_d` | 439 passed, 0 failed (403 ignored) |
| `cargo test --locked -p cs_app --test cs_app_tests_e` | 12 passed, 0 failed (11 ignored) |
| `cargo test --locked -p cs_app --test cs_app_tests_c -- world:: --include-ignored` | 158 passed, 3 failed — all three pre-existing evidence-harness preconditions (see below) |
| `cargo test --locked -p cs_app --test cs_app_tests_c -- accept_f18_b_ --include-ignored` | 45 passed, 0 failed |
| `cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t424_ accept_t420_ accept_t1424_ --include-ignored` | 23 passed, 0 failed |
| `cargo test --locked -p cs_app --test cs_app_tests_a -- accept_t1430_ --include-ignored` | 2 discovered, 2 passed |
| `cargo test --locked -p cs_app --test cs_app_tests_b -- accept_playtest_retail_retail_c1c_area_and_bloodhawk_mesh_spawn_and_capture --include-ignored` | 1 passed (retail, GPU capture; screenshots in `private/`) |

The three `world::*` failures under `--include-ignored` are the
pre-existing `CS_EVIDENCE_DIR is not set` evidence-harness preconditions
recorded in the #1424 finding, not this task's: they refuse to run outside the
evidence sequence in their own module docs. Measured on the reviewed commit's
tree.

## Honest limits

* The phantom time-of-impact itself is not re-measured here; that belongs to
  F10-D.1 and `accept_f18_b_*`, which are unchanged and green. What changes is
  which shape reaches parry and how many allocations there are, both observed
  directly by the tests above.
* "One allocation" is pinned through Avian's/parry's `SharedShape` arc
  (`Arc::ptr_eq` and `strong_count`). A future Avian that stored shapes
  differently would need this probe re-expressed, which the test says in its
  own doc.
* The trigger-volume layout reaches the same derived shape as the solid one;
  the *body-less* fact is still held by `world::trigger` and #401's recorded
  measurement, not re-measured here.
* Artifact size decreased (fewer `TriMesh` allocations); no frame-time
  measurement was taken, and none is claimed.
