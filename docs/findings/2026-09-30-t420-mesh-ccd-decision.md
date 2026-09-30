# T420: swept CCD vs collider placement — the mesh-tunnelling decision

Date: 2026-09-30. Task: #420 `F18-B-followup-mesh-ccd` — "Decide how world
collision detects a body that outruns its own sampling", filed by F18-B (#86).
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no
evidence report required, nothing `verified_original`. Every engine statement
below was read from the pinned sources and then **measured**, twice: directly
against parry 0.27 and in the real headless composition
(`cs_app::asset_stack::headless_app` + `cs_app::physics` fixed-rate adapter +
`cs_app::world::spawn_swept_probe`).

Pinned pair: `bevy 0.19.1` / `avian3d 0.7.0` / `parry3d 0.27.0`,
`SubstepCount(1)`, 120 Hz fixed, gravity zero. Probe: the production
`spawn_swept_probe` body (0.5 m box, `SweptCcd` + `SpeculativeMargin::ZERO`),
400 m/s = 3.33 m per tick; wall: an open box 1 m thick — a tick outruns it.

## The corrected root cause

F18-B's measurement was right and its attribution was wrong. The gap is real;
"parry has no `TriMesh` case in `cast_shapes`" is not the cause.

* **Parry casts fine against `TriMesh`.** `TriMesh::as_composite_shape()`
  returns `Some(self)` (`parry3d-0.27.0/src/shape/shape.rs:1141`), so
  `DefaultQueryDispatcher::cast_shapes` and `cast_shapes_nonlinear` both
  route a trimesh through the composite-shape branch — they never reach
  `Err(Unsupported)`. Measured directly on parry 0.27: a cuboid swept at
  400 m/s into a trimesh wall returns `Ok(Some(ShapeCastHit))`.
* **The actual defect is Avian's pair query.** `solve_swept_ccd` iterates the
  contact-graph neighbours and resolves each to its body with
  `SweptCcdBodyQuery`
  (`avian3d-0.7.0/src/dynamics/ccd/mod.rs::compute_ccd_toi`'s caller, the
  query at lines 503–513). That query requires
  `collider: &'static Collider` **on the body entity**. A collider attached
  to a child node — which is what `ColliderConstructorHierarchy` produces, and
  the only way `TrimeshFromMesh` can attach — makes `get_unchecked` fail and
  the pair is silently skipped. No shape cast is ever attempted.

Measured 2×2 on the same probe, speed and wall span:

| collider | placement | 400 m/s probe |
| --- | --- | --- |
| trimesh | on the body entity | **stopped at x ≈ −0.75** (wall near face) |
| trimesh | on a child node | **tunnels**, no contact |
| cuboid | on a child node | **tunnels**, no contact |
| cuboid | on the body entity | **stopped at x ≈ −0.75** |

The cuboid-on-child row is the decisive measurement: a support map, the
shape class the F18-B finding named as working, tunnels identically. The
discriminant is *where the `Collider` component sits*, never the shape.

The rule is symmetric: the swept body also needs `Collider` on its own
entity (the same query fetches it), so a `SweptCcd` body whose colliders all
live on children never sweeps at all.

## Is a newer pinned version the answer? — No.

* `avian3d` on crates.io: **0.7.0 is the newest release** — the pinned
  version already is the latest. There is nothing to bump to.
* `parry3d` on crates.io: latest is 0.31.1, but (a) `avian3d 0.7.0` requires
  `parry3d ^0.27`, so it cannot be selected without an avian release, and
  (b) it would change nothing: the defect is in avian's ECS query, and
  parry 0.31's dispatcher still routes `TriMesh` through the same composite
  branch (verified in its source — same structure, no new `TriMesh` arm in
  `cast_shapes`, which it doesn't need).
* Upstream avian `main` has already rewritten swept CCD entirely
  (Box2D-style continuous solving; `CcdBodyQuery` iterates
  `RigidBodyColliders` instead of requiring `&Collider` on the body). The
  class of bug is fixed upstream but **unreleased**. When an avian release
  containing it ships and the pin moves, the pinning tests are the
  re-measure signal: they assert the current behaviour and fail the moment
  it changes.

## The decided configuration

**World colliders live on their rigid-body entity.** Concretely, for
mesh-derived world objects: one entity carries `RigidBody::Static` +
`Mesh3d` + `ColliderConstructor::TrimeshFromMesh` — the same uploaded mesh
drives presentation and collision, the derived `Collider` lands on the body
entity itself, and swept CCD sees it. Equivalently `Collider::trimesh` /
`trimesh_from_mesh` written on the body entity. This is an *entity layout*
rule, not a geometry proxy: every stored triangle still reaches the
collider (asserted in the test), so F18 non-negotiable behavior 1 is
honoured the same way F18-B already honours it — by substitution being
visible rather than by substituting.

Everything else is unchanged, for measured reasons:

* **`SpeculativeMargin::ZERO` stays on swept layers.** A nonzero margin
  does stop the miss (measured: `SpeculativeMargin(10)` clamps the same
  probe at x ≈ −0.76 against a child-node trimesh, because the speculative
  narrow phase goes through parry's `contact_manifolds`, which *does* have
  a `TriMesh` case). But it does so by predicting contacts metres ahead of
  the body — the "globally inflated hitbox" F23 non-negotiable behavior 3
  forbids, which is exactly why production bodies zero it
  (`cs_app::physics::body`). Not the fix.
* **`SubstepCount(1)` stays.** Measured at `SubstepCount(2)`, `(4)` and `(8)`:
  the probe still tunnels a child-node collider — substeps subdivide the
  solver, not the detection pipeline, so a skipped pair stays skipped at any
  rate, and each substep costs a full solver pass. Not the fix, at any price.
* **No approach-speed cap.** The restriction would have to cover every
  moving thing the game simulates (aircraft in a dive exceed 150 m/s;
  projectiles are faster still) and is unverifiable against unimported
  geometry. Not the fix.
* **No pin bump and no `[patch]` fork.** Nothing to bump to today; forking
  avian to backport the upstream CCD rewrite is a maintenance liability far
  larger than the layout rule. Re-measure when upstream releases.

The one-line rule for every future body-spawning path: **a body that swept
bodies must stop against — or that sweeps itself — carries at least one
`Collider` on its own entity.** For single-collider objects that is the
collider. A body with colliders on child nodes stays fully swept-eligible
as long as the body entity itself also has one (the query only checks the
body; the cast still uses the *child* collider's own shape) — so no dummy
geometry is needed as long as any real collider sits on the root.

## Residual limitation and follow-ups

* A rigid body whose colliders **all** live on descendant entities remains
  invisible to swept CCD on the pinned engine — any shape, any speed above
  the discrete sampling rate. Known producers of that layout:
  `ColliderConstructorHierarchy` colliders (the `TrimeshFromMesh` path
  F18-B currently builds) and any multi-part body that puts *all* part
  colliders on children (aircraft-part colliders, capital-ship subsystems —
  the F11-C/F35 families must be checked when they land). Filed as a
  follow-up: the layout rule has to become an enforced invariant or these
  stay permanently swept-invisible until upstream releases the fix.
* `cs_app::asset_stack::spawn_static_mesh_collider` itself produces the
  child-node layout (its `MeshColliderNode` is body + mesh-node child). It
  stays as-is here — its contract belongs to #333 and its callers decide
  which layout they need — but the decided rule means world import must not
  use the parent+child arrangement for swept-relevant geometry. The single-
  entity form is a strict reduction of it.
* The `Sensor`/swept interaction measured by F18-A (task #401) is unchanged
  by this decision: it was measured with colliders on body entities.
* Task #401 is a sibling of this question, not a duplicate: the sensor stop
  happens through the same swept path the layout rule repairs, so a
  sensor on a child node is invisible to it too.

## What the F18-B records need when they land

F18-B's branch (`rally/86-…`, in review when this was written) contains the
two artifacts this task was asked to update; they do not exist on `main`
yet. When it merges, `docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`
and `crates/cs_app/tests/world/import.rs::accept_f18_b_a_tunnelling_body_misses_mesh_geometry_which_is_a_pinned_engine_limit`
need:

1. The attribution paragraph replaced with the corrected one above (the
   collider-on-child-vs-body finding, with the cuboid-on-child measurement).
2. `spawn_object`'s mesh path changed to the decided layout — one entity
   `RigidBody::Static` + `Mesh3d` + `ColliderConstructor::TrimeshFromMesh`,
   i.e. drop the parent body + child node split. The triangle-count and
   fingerprint assertions are unaffected.
3. The pinned test renamed/re-asserted: with the layout applied the probe
   **is** stopped at x ≈ −0.75, so the test becomes the regression guard
   ("a tunnelling body must not miss mesh geometry") — or it is kept
   asserting the child-node limitation explicitly in the manner of the
   `accept_t420_` arms here. The reviewer of #86 was given this correction
   in a task note while the review was still open, so the fix may land
   there first.

## Measured evidence

* `crates/cs_app/tests/accept_t420_mesh_ccd.rs` — 7 tests, all passing:
  - `accept_t420_a_mesh_collider_on_a_child_node_is_invisible_to_swept_ccd`
    (child trimesh via production `spawn_static_mesh_collider`: tunnels)
  - `accept_t420_a_cuboid_on_a_child_node_is_ignored_the_same_way`
    (child cuboid: tunnels — the attribution correction)
  - `accept_t420_a_mesh_collider_on_the_body_entity_stops_the_swept_probe`
    (decided layout: stopped, all 4 stored triangles in the collider)
  - `accept_t420_a_direct_trimesh_collider_on_the_body_also_stops_the_probe`
  - `accept_t420_a_cuboid_on_the_body_entity_stops_the_same_probe`
  - `accept_t420_substeps_do_not_make_a_child_node_collider_visible`
    (`SubstepCount(8)`: still tunnels)
  - `accept_t420_the_probe_is_a_swept_body` (fixture guard)
* Direct parry 0.27 check (scratch crate, `parry3d = "=0.27.0"`):
  `cast_shapes`/`cast_shapes_nonlinear` cuboid-vs-trimesh →
  `Ok(Some(ShapeCastHit))`; `Err(Unsupported)` never occurs for trimesh.
* Speculative-margin check (same harness, `SpeculativeMargin(10)`):
  probe clamped at x ≈ −0.76 with a recorded contact — works, rejected on
  spec grounds.
* Version check: crates.io API — `avian3d` latest 0.7.0, `parry3d` latest
  0.31.1; `avian3d 0.7.0` dep `parry3d ^0.27`; parry 0.31.1
  `default_query_dispatcher.rs` has the same composite-path structure;
  upstream avian `main`'s `src/dynamics/ccd/mod.rs` contains the rewritten
  `CcdBodyQuery` over `RigidBodyColliders` (unreleased).

Commands run locally:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # exit 0
cargo test --workspace --locked -- accept_t420_ --include-ignored # 7 run, 7 passed
```

## Sources

Pinned `avian3d-0.7.0`, `parry3d-0.27.0` sources in the local cargo
registry; crates.io API for the published version sets; upstream
`avianphysics/avian` `main` for the CCD rewrite status; the F18-A and F18-B
findings and task notes on #86/#333. No original data was read and no
original behavior is claimed.
