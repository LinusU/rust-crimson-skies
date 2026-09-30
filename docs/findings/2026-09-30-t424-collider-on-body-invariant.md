# T424: the collider-on-body rule, as an invariant

Date: 2026-09-30. Task: #424
`T420-FOLLOWUP-COLLIDER-ON-BODY-INVARIANT` — "Make the collider-on-body rule an
invariant across every body-spawning path", filed by task #420.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no evidence
report required, nothing `verified_original`.

Pinned pair: `bevy 0.19.1` / `avian3d 0.7.0` / `parry3d 0.27.0`,
`SubstepCount(1)`, 120 Hz fixed, gravity zero.

## The rule

**A rigid body that swept bodies must stop against — or that sweeps itself —
carries at least one `Collider` on its own entity.**

Task #420 measured *why* this is a rule and not a preference, and the
measurement is worth restating because the obvious explanation is wrong.
`solve_swept_ccd` iterates the contact-graph neighbours and resolves each to
its body with `SweptCcdBodyQuery`
(`avian3d-0.7.0/src/dynamics/ccd/mod.rs`), whose `collider: &'static Collider`
field is read off the **body** entity. A body whose colliders all live on
descendants fails that query; the pair is skipped and no shape cast is ever
attempted.

The consequence is symmetric and both directions matter for this workspace:

* a static body a swept body must **stop against** is invisible, so the swept
  body flies through it at any speed above the discrete sampling rate;
* a `SweptCcd` body whose own colliders are on children never **sweeps** at
  all, so it also passes through thin geometry it should have detected.

Shape is irrelevant. The decisive measurement from #420 is a *cuboid* on a child
node, which tunnels identically to a trimesh: placement, not shape.

## What changed

### `crates/cs_app/src/asset_stack.rs`

* **`spawn_static_mesh_collider_on_body`** (new) — the single-entity layout: one
  entity carrying `RigidBody::Static` + `Mesh3d` +
  `ColliderConstructor::TrimeshFromMesh` + the layer membership + the authored
  transform. Avian's `init_collider_constructors` inserts the derived `Collider`
  **on the entity that holds the constructor**, so the collider lands on the
  body and the sweep can resolve it. This is a strict reduction of
  `spawn_static_mesh_collider`: same upload, same constructor, same every
  stored triangle, same membership, same transform — one entity instead of a
  body and a child.
* **`SweptInvisible`** (new) — a marker carrying a `reason: &'static str`, put
  on the body that `spawn_static_mesh_collider` returns. The hierarchy layout is
  a genuine, deliberate, *declared* exception: it is the F00-A #333 contract and
  the only way to express per-descendant constructor configurations
  (`ColliderConstructorHierarchy::with_constructor_for_name`). What it is not is
  silently swept-invisible.
* **`swept_invisible_bodies`** / **`undeclared_swept_invisible_bodies`** (new) —
  the rule as a query. The first reports every body with no `Collider` of its
  own together with the descendants that hold its colliders and its declaration;
  the second filters to the bodies nobody declared, which is the invariant.

### `crates/cs_app/src/world/spawn.rs`

`spawn_mesh_collider` now uses the single-entity layout and returns one
`Entity`, so a `FromMesh` world object is one entity: presentation, collider and
static body together. `SpawnedCollider::entity` and `SpawnedCollider::body` are
therefore the same entity for a mesh object, and still two for a cuboid (whose
presentation is its own entity). The `body` field is kept because a consumer
asking "where is the body" should not have to know the two coincide, and
because a cuboid's presentation and collider genuinely are different entities.

### `crates/cs_app/tests/world/{import,residency}.rs`

Three F18-B assertions described the old layout and are corrected, not deleted:

| before | after |
| --- | --- |
| `assert_ne!(collider.body, collider.entity)` — "the derived collider hangs off its static body, which is a separate entity" | `assert_eq!` — a body with no collider is skipped by `SweptCcdBodyQuery` |
| `assert!(!app.world().get::<Collider>(body).is_some())` — "the collider is on the node, not the body" | the collider **must** be on the body |
| a mesh object owns "at least a body and a node" (`>= 2` entities) | exactly one entity |

### The F18-B records

Per #420's "What the F18-B records need when they land":
`docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md` had its
attribution paragraph replaced with the measured one, and
`accept_f18_b_a_tunnelling_body_misses_mesh_geometry_which_is_a_pinned_engine_limit`
became `accept_f18_b_a_tunnelling_body_is_stopped_by_the_mesh_geometry_it_flys_at`
— a regression guard rather than a limitation pin, which is the form #420
prescribed once the layout landed.

## The measurement, re-run here

The 2x2 from #420, reproduced through the production composition
(`asset_stack::headless_app` + `PhysicsAdapterPlugin` + the production
`spawn_swept_probe` 400 m/s `SweptCcd` body, 3.33 m of travel per tick, against
a 1 m wall, `SpeculativeMargin::ZERO`):

| collider | placement | 400 m/s probe |
| --- | --- | --- |
| trimesh | on the body entity | **stopped at the wall's near face** |
| trimesh | on a child node | tunnelled, no contact |
| cuboid | on a child node | tunnelled, no contact |
| cuboid | on the body entity | **stopped at the wall's near face** |

Pinned by `accept_t424_a_collider_on_the_body_is_what_a_swept_body_stops_against`,
which asserts both arms in one test so a future engine that drops the
requirement cannot pass by accident. The F18-B flight test measures the same
thing through the real world import: the probe is now clamped at the arch's near
face with the hangar contact recorded, where before it ended at `x = 0.62`
tunnelling through.

## The paths, and what each one does

Every production path that spawns a `RigidBody` in the workspace, and its
standing under the rule:

| path | standing |
| --- | --- |
| `physics::body::spawn_body` | holds it. `Collider::cuboid` is inserted on the entity that also carries the `RigidBody`, for every `BodyMode` and both `ShapeClass` values. Pinned by `accept_t424_every_body_spawn_body_produces_carries_a_collider_on_its_own_entity`, which covers dynamic/kinematic/static and solid/sensor, because a kinematic body is released to dynamic by `set_body_mode` and must be stoppable before *and* after. |
| `world::spawn::spawn_object` (cuboid) | holds it. One entity, `RigidBody::Static` + `Collider::cuboid` + layers. |
| `world::spawn::spawn_object` (`FromMesh`) | holds it **as of this task**; did not before. Now one entity through `spawn_static_mesh_collider_on_body`. Pinned by `accept_t424_the_world_import_path_leaves_no_body_invisible_to_a_sweep`, which also asserts the report really produced four colliders so the empty audit cannot pass on a world that spawned nothing. |
| `asset_stack::spawn_static_mesh_collider` | **declared exception.** The hierarchy layout, kept for the #333 contract and for per-descendant constructors. Its body carries `SweptInvisible` with a reason naming the mechanism and the call site that *does* hold the rule. |
| `synthetic::SyntheticSceneBuilder` | holds it. `Collider::cuboid` on the body entity. |
| `physics::fixture` | holds it. Same. |
| `world::fixture::spawn_probe` | holds it. Same. The three fixture paths are listed because they are production bootstrap code in this crate's own sense, not because gameplay reaches them. |

Two shapes deliberately have **no** body and so satisfy the rule vacuously, and
a test says so: a role-`None` world object (presented, never collided) and an
object whose mesh nobody supplied. Both are pinned by
`accept_t424_a_presentation_only_object_has_no_body_to_hide_a_collider_on`,
which exists so a future change cannot quietly give one of them a colliderless
body.

### The multi-part case

A body with colliders on children is **not** lost, as long as the body entity
also carries one real collider: `SweptCcdBodyQuery` reads the collider off the
body, and the cast then uses the child collider's own shape. So a multi-part
body (the F11-C aircraft-part colliders, the F35 capital-ship subsystems) needs
only one real part on its root — **no dummy geometry**, which is the point worth
stating, because the obvious alternative to "invisible to sweeps" is a
placeholder box and a placeholder box is exactly the invented geometry F18
non-negotiable behavior 1 forbids. Pinned by
`accept_t424_a_multipart_body_needs_only_one_real_collider_on_its_root`, which
builds the two-part case and then flies a 400 m/s probe at it.

## What is *not* claimed

* **Not a version bump.** avian3d 0.7.0 is the newest release; parry3d 0.31.1 is
  unselectable (avian pins `^0.27`) and irrelevant, the defect being avian-side.
  Upstream avian `main` has rewritten swept CCD to iterate `RigidBodyColliders`
  and so no longer needs `&Collider` on the body — the class of bug is fixed
  there and **unreleased**.
* **Not a re-measure of retail behaviour.** Everything here is a property of the
  pinned engine and of our entity layout. No original byte was read and nothing
  is `verified_original`.
* **The rule is ours, not the original's.** It constrains this engine's
  requirement on this pinned pair. It says nothing about how the 2000 game
  arranged collision.
* **No geometry was invented and no F18 non-negotiable was traded.** The
  body-entity layout is a reduction of the old one: `accept_t424_the_body_layout_keeps_every_stored_triangle_and_the_authored_transform`
  compares the two colliders built from the same upload and asserts they carry
  the same triangles, and a cuboid-substitution mutation was run against the
  whole suite to confirm a "fix" by proxy shape would fail (see below).

## The re-measure signal

`accept_t424_a_collider_on_the_body_is_what_a_swept_body_stops_against` asserts
the child-layout arm still tunnels. When a released Avian drops the
requirement, that arm fails. That failure means **re-measure and re-decide the
rule**, not "regression" and not "delete the test": once the engine resolves
bodies through `RigidBodyColliders`, the layout stops mattering and both layouts
become correct, at which point the hierarchy path could stop declaring an
exception. The same applies to
`import::accept_f18_b_a_tunnelling_body_is_stopped_by_the_mesh_geometry_it_flys_at`
only in the weaker sense that its subject (mesh sweeps) may gain new limits.

## Test sensitivity (mutation matrix)

Each mutation was applied to production source, the selection was run, the
source was restored byte-identically. None of the probes is committed.

| mutation | tests that failed |
| --- | --- |
| `spawn_mesh_collider` reverts to `spawn_static_mesh_collider` (parent body + child node) | `accept_t424_the_world_import_path_leaves_no_body_invisible_to_a_sweep`; `import::..._a_tunnelling_body_is_stopped_by_the_mesh_geometry_it_flys_at`, `import::..._a_mesh_collision_is_the_geometry_the_object_draws_and_keeps_its_opening` |
| `swept_invisible_bodies` returns an empty list | `accept_t424_..._the_audit_reports_a_declared_exception_and_nothing_else`, `..._an_undeclared_child_node_collider_is_reported_rather_than_ignored` |
| the hierarchy body stops carrying `SweptInvisible` | `accept_t424_..._the_audit_reports_a_declared_exception_and_nothing_else` |
| `spawn_static_mesh_collider_on_body` degenerates to `ColliderConstructor::Cuboid` | `accept_t424_..._the_body_layout_derives_the_collider_onto_the_body_itself`, `..._keeps_every_stored_triangle_...`, `..._a_collider_on_the_body_is_what_a_swept_body_stops_against`; five `accept_f18_b_` tests |
| the body-entity layout stops carrying the authored `Transform` (review fix) | `accept_t424_..._the_body_layout_honours_a_scaled_placement_without_simplifying_the_mesh` — and nothing else, which is why that test exists |
| `collider_descendants` pops from the back, so it is depth first (review fix) | `accept_t424_..._the_audit_reports_every_collider_descendant_in_a_stable_order` |

The last row is the one worth noting: a "fix" that made world geometry
sweep-visible by replacing the trimesh with a box would satisfy the sweep
assertion and fail the triangle assertions. Geometry and layout are pinned
separately, so neither can be traded for the other.

The two review rows are gaps the reviewer found rather than a defect in the
layout: the scale of a mesh object's placement was claimed in prose ("same
every stored triangle, same layer membership, same `transform`") and pinned
nowhere on the new layout, and the audit's report order was documented as
breadth first while the walk was depth first. Both were measured, both failed
under their mutation, and both are now properties of the suite rather than of a
paragraph.

## Evidence

Ordinary build/test only. No `CS_GAME_DIR` read, no evidence report required or
produced, no original behaviour claimed. Every engine statement was read from
the pinned registry sources and then measured in-engine:

* `avian3d-0.7.0/src/dynamics/ccd/mod.rs` — `SweptCcdBodyQuery` really carries
  `collider: &'static Collider`;
* `avian3d-0.7.0/src/collision/collider/backend.rs` —
  `init_collider_constructors` inserts the collider on the constructor's own
  entity; `init_collider_constructor_hierarchies` iterates
  `children.iter_descendants` and never touches the root;
* `parry3d-0.27.0/src/shape/shape.rs:1141` — `TriMesh::as_composite_shape` really
  returns `Some(self)`, which is what makes the *old* F18-B attribution wrong;
* `crates/cs_app/tests/accept_t424_collider_on_body.rs` — 12 tests, all passing;
* the 2x2 and the F18-B flight path, measured through the production
  composition as above.

Commands run locally:

```sh
cargo fmt --all -- --check                                                  # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                             # exit 0
cargo test --workspace --locked -- accept_t424_ --include-ignored           # 12 run, 12 passed
cargo test --workspace --locked -- accept_f18_ --include-ignored            # 33 run, 33 passed
```

## Review findings (2026-09-30, reviewer `bunny-alpha-1`)

**The implementer and the reviewer are the same agent identity**
(`bunny-alpha-1`), in two different sessions; this review session started with
no memory of the implementation and re-derived the engine claims from the
pinned sources. It is still **not** independent evidence, and nothing in this
record should be read as a second opinion on the layout decision itself. What
was checked, and what it changed:

* **The three engine claims were re-read in the pinned sources, not accepted.**
  `SweptCcdBodyQuery`'s `collider: &'static Collider` field is read off the body
  entity and the neighbour is resolved with `bodies.get_unchecked(entity2)`
  (`avian3d-0.7.0/src/dynamics/ccd/mod.rs:503-513`, `575`);
  `init_collider_constructors` inserts the derived collider on the constructor's
  own entity (`backend.rs:264-315`) while `init_collider_constructor_hierarchies`
  walks descendants only; and `TriMesh::as_composite_shape` does return
  `Some(self)` (`parry3d-0.27.0/src/shape/shape.rs:1141-1143`), so the F18-B
  attribution this task replaces was wrong about the library, not just about the
  layout.
* **A claim in prose that no test held: the scale.** "Same every stored
  triangle, same layer membership, same `transform`" was asserted only for the
  triangle count, and the world path really does hand a decomposed (possibly
  non-uniform) scale to the new layout. Dropping the `Transform` from
  `spawn_static_mesh_collider_on_body` passed all ten original tests and would
  have silently collided at the wrong size — the exact trade F18 non-negotiable
  behavior 1 forbids. `accept_t424_the_body_layout_honours_a_scaled_placement_without_simplifying_the_mesh`
  now reads `shape_scaled()` (what the narrow phase collides against) and
  requires every vertex to be an uploaded corner scaled per axis, identically
  for both layouts.
* **The audit's report order was documented as breadth first and implemented as
  depth first**, and no test held either. The walk now uses a queue, and
  `accept_t424_the_audit_reports_every_collider_descendant_in_a_stable_order`
  pins a three-holder, two-level hierarchy, which is the case where the two
  orders disagree.
* **Checked and left alone, deliberately:** `SpawnedCollider::body` still
  exists and now equals `entity` for a mesh object — a cuboid's presentation and
  collider really are different entities, so a consumer asking "where is the
  body" should not have to know the two coincide; and the audit
  (`swept_invisible_bodies` / `undeclared_swept_invisible_bodies`) is production
  API called only by tests. A per-frame Bevy system that reads it was considered
  and not taken: the invariant belongs to a test that fails loudly, not to a
  warning nobody reads in a release build, and the world already has a real
  runtime cost.
* **Overlap with #420 checked.** #420's branch adds only
  `crates/cs_app/tests/accept_t420_mesh_ccd.rs` and its own decision record, so
  the two branches cannot conflict textually; its measured 2x2 and this task's
  re-measure agree row for row, and its "what the F18-B records need" list is
  what this task implemented.

## Sources

Pinned `avian3d-0.7.0` and `parry3d-0.27.0` in the local cargo registry; task
#420's decision record
`docs/findings/2026-09-30-t420-mesh-ccd-decision.md`; the F18-A and F18-B
findings and the task notes on #86/#333/#401/#420. No original data was read and
no original behavior is claimed.
