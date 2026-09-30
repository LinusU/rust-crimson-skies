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
  does stop the miss — the speculative narrow phase *does* have a `TriMesh`
  case, and it is the swept query that skips the pair, not the narrow phase.
  But it does so by predicting contacts ahead of the body, and the margin
  needed to matter is not a small tolerance: measured on the fixture above,
  a margin of 1 m still tunnels and the threshold is between 1 m and 2 m,
  against a 3.33 m tick — a hitbox grown by metres, which is the "globally
  inflated hitbox" F23 non-negotiable behavior 3 forbids and which is exactly
  why production bodies zero it (`cs_app::physics::body`). Not the fix. Pinned
  as a rejected alternative by
  `accept_t420_a_speculative_margin_stops_what_the_swept_query_skips`, so the
  cost recorded here stays checkable rather than remembered.
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
  `ColliderConstructorHierarchy` colliders and any multi-part body that puts
  *all* part colliders on children (the F11-C aircraft-part and F35
  capital-ship families, when they land — #424's
  `accept_t424_a_multipart_body_needs_only_one_real_collider_on_its_root`
  already pins that one real collider on the root keeps the whole body
  swept-visible). #424 (`T420-FOLLOWUP-COLLIDER-ON-BODY-INVARIANT`, merged
  at `b443283`) made the layout rule an enforced invariant:
  `cs_app::asset_stack::undeclared_swept_invisible_bodies` audits every
  production body-spawning path, so a new all-on-children body fails a test
  rather than shipping silently.
* `cs_app::asset_stack::spawn_static_mesh_collider` still produces the
  child-node layout (its `MeshColliderNode` is body + mesh-node child) and
  stays as the multi-node helper; world import uses the single-entity
  `spawn_static_mesh_collider_on_body` instead. The audit above is what
  stops a caller from reintroducing the arrangement for swept-relevant
  geometry.
* The `Sensor`/swept interaction measured by F18-A (task #401) is unchanged
  by this decision: it was measured with colliders on body entities.
* Task #401 is a sibling of this question, not a duplicate: the sensor stop
  happens through the same swept path the layout rule repairs, so a
  sensor on a child node is invisible to it too.
* Fixture notes, measured while reviewing. Both are properties of this
  open-box wall mesh — two zero-thickness quads with no thickness between them
  — and neither is about swept CCD:
  * A discrete body that penetrates *between* the two faces is wedged inside
    the slab rather than held at the near face, which is why the discrete-path
    control above uses the cuboid arm.
  * The swept clamp itself is exact — the probe is placed at x = −0.7496 on
    the clamping tick, the wall's near face less the probe's 0.25 m half — but
    Avian's swept CCD only truncates the tick's translation and leaves the
    body's velocity alone, so from the next tick the discrete narrow phase
    resolves the remaining contact on a zero-thickness face and the body
    creeps *along* it (y and z both rise ~0.05 m per tick, ending near
    x = −0.92). This is why the assertions here are "stopped before the wall"
    rather than an exact position, and why the x ≈ −0.75 quoted for the
    clamping tick is not where the body sits 14 ticks later.

## What the F18-B records needed, and where the correction landed

F18-B's branch (`rally/86-…`) was still in review when this was written, so
the correction was recorded here and handed to its reviewer in a task note.
F18-B merged at `cf27e3d` with the original attribution intact, and then
**#424 (`T420-FOLLOWUP-COLLIDER-ON-BODY-INVARIANT`) landed the whole fix
before this decision record did** (`8fd3762`…`b443283`):

1. `spawn_static_mesh_collider_on_body` builds the decided layout — one
   entity `RigidBody::Static` + `Mesh3d` +
   `ColliderConstructor::TrimeshFromMesh` — and `spawn_object`'s mesh path
   uses it; the parent body + child node split is gone from world import.
2. `swept_invisible_bodies` / `undeclared_swept_invisible_bodies` in
   `cs_app::asset_stack` are the enforced audit: the collider-on-body rule
   is now an invariant, checked by `accept_t424_collider_on_body.rs`, not a
   paragraph somebody has to remember.
3. The F18-B finding's engine-limitation section carries the corrected
   cause, and the pinned test flipped to the regression guard
   `import.rs::accept_f18_b_a_tunnelling_body_is_stopped_by_the_mesh_geometry_it_flys_at`
   — the probe **is** stopped at the wall face now.

What remains on this branch is the decision record itself (which both
records above cite) and the `accept_t420_` measurement suite that pins the
engine behaviour the rule is built on: the 2x2 placement matrix, the
discrete-path control, the two rejected fixes (substeps and a speculative
margin) and the fixture guards.

## Measured evidence

* `crates/cs_app/tests/accept_t420_mesh_ccd.rs` — 9 tests, all passing:
  - `accept_t420_a_mesh_collider_on_a_child_node_is_invisible_to_swept_ccd`
    (child trimesh via production `spawn_static_mesh_collider`, asserted
    attached to the body: tunnels)
  - `accept_t420_a_child_node_collider_still_stops_a_discrete_body`
    (same wall, discrete 30 m/s probe: stopped — attached but invisible to
    swept CCD, not a missing collider)
  - `accept_t420_a_cuboid_on_a_child_node_is_ignored_the_same_way`
    (child cuboid of the *same* 1 m span, asserted attached: tunnels — the
    attribution correction)
  - `accept_t420_a_mesh_collider_on_the_body_entity_stops_the_swept_probe`
    (decided layout, through production
    `spawn_static_mesh_collider_on_body`: stopped, all 4 stored triangles in
    the collider)
  - `accept_t420_a_direct_trimesh_collider_on_the_body_also_stops_the_probe`
  - `accept_t420_a_cuboid_on_the_body_entity_stops_the_same_probe`
  - `accept_t420_substeps_do_not_make_a_child_node_collider_visible`
    (`SubstepCount(2)`, `(4)` and `(8)`: still tunnels)
  - `accept_t420_a_speculative_margin_stops_what_the_swept_query_skips`
    (the rejected margin alternative, pinned so its cost stays checkable)
  - `accept_t420_the_probe_is_a_swept_body` (fixture guard: `SweptCcd` *and*
    a `Collider` on the probe, the other half of the symmetric rule)
* Direct parry 0.27 check (scratch crate, `parry3d = "=0.27.0"`):
  `cast_shapes`/`cast_shapes_nonlinear` cuboid-vs-trimesh →
  `Ok(Some(ShapeCastHit))`; `Err(Unsupported)` never occurs for trimesh.
* Speculative-margin sweep, measured on the fixture above: `SpeculativeMargin`
  0 and 1 both tunnel (x ≈ 34.67), 2 stops the probe at x ≈ −1.04, and 3 m and
  above all stop it at x ≈ −0.77. It works, and it is rejected on spec grounds.
  The committed `accept_t420_a_speculative_margin_stops_what_the_swept_query_skips`
  pins the working end of that sweep; the numbers above are the same
  measurement, recorded rather than only asserted.
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
cargo test --workspace --locked -- accept_t420_ --include-ignored # 9 run, 9 passed
```

## Sources

Pinned `avian3d-0.7.0`, `parry3d-0.27.0` sources in the local cargo
registry; crates.io API for the published version sets; upstream
`avianphysics/avian` `main` for the CCD rewrite status; the F18-A (#401) and
F18-B (#86) findings and task notes, plus the F00-A #333 asset-stack record
this branch's hierarchy arm depends on. No original data was read and no
original behavior is claimed.
