# F20-C.05: the spawn wiring for a scene node's collider

Date: 2026-10-02. Task: F20-C.05 "Insert `NodeColliderPresence` and an Avian
collider for a scene node with an authored `CollisionRole::Collider`" (#510).
Spec: `specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-C`, non-negotiable behavior 3, acceptance AC01. Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md` (and `docs/contracts/IDENTITY-CONTENT.md` for
the generation-stamped binding the load already stamps). Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no render, no audio, so no
`private/evidence/` report is produced.

## What this stage closes

F20-C.04 built the collision-side consumer and left its opt-in unwired, on
purpose. `NodeColliderPresence` is **opt-in**: `apply_collider_presence` queries
`With<NodeColliderPresence>`, and a node without the record is invisible to the
policy, so a clip hiding such a node left the invisible obstacle F20-A's rule
exists to prevent. F20-C.04's finding stated the cost out loud — "a clip-hidden
node whose spawner never inserted the record keeps colliding" — and named the
constraint the wiring had to honour:

> the presence record, the Avian `Collider` and the clip's
> `NodeAnimatedVisibility` all have to live on the **same** entity, because the
> pass reads all three from one entity.

Two production facts made the gap total, and both are still true of the
content model: `crates/cs_app/src/scene.rs` attached **no** Avian collider to a
scene node at all, and nothing inserted the record.

## The decision: the load owns a node's collider

`opt_node_into_collision_policy` runs in the socket-binding loop of
`load_airframe_scene` — the same loop that binds `PartBinding`, resolved through
the same generation-stamped `SceneImport` — so the record is on the verified
path and a superseded generation releases it with the node it belongs to. The
animation path is not involved: it keeps publishing its verdict and the physics
layer keeps reading it (`crates/cs_app/src/animation/` is untouched).

Three sub-decisions, and the candidates each rejected.

### 1. The collider is on the node's own entity, which is the body

The task asked for a decision here. The chosen layout is **collider-on-body**,
for two independent reasons that point the same way:

* the **collider-on-body rule** (task #424,
  `docs/findings/2026-09-30-t424-collider-on-body-invariant.md`): Avian's
  `solve_swept_ccd` resolves candidates through `Query<(&Collider, &ColliderOf)>`
  (`avian3d-0.7.0/src/dynamics/ccd/mod.rs:526`), so a body whose colliders
  live on descendants is skipped by a swept body and the sweep passes straight
  through it;
* the **same-entity constraint** F20-C.04 documented: the pass reads the
  presence record, `NodeAnimatedVisibility` and the `Collider` from one entity.

Rejected: a child entity holding the collider under a body on the node. It
satisfies the first rule and breaks the second, and probe P7 below shows the
acceptance test catches exactly that layout.

So the node entity gains `RigidBody::Static`, `Position`, `Rotation`, `Collider`,
`AvianCollisionLayers` and `cs_app::physics::BodyLayer`.

### 2. No `Transform` and no `GlobalTransform` on a scene node

F18's world path puts the collider's **pose** in a `Transform` on the collider
entity and lets Avian scale the collider from it
(`crate::world::spawn::spawn_cuboid_collider`). A scene node cannot: its one
visual pose owner is `NodeVisualTransform`, which is deliberately *not* a
`GlobalTransform` component, and the transform-propagation systems overwrite a
`GlobalTransform` from a `Transform` on the same entity — measured in
`docs/findings/2026-09-30-f18-b-followup-sheared-world-object-placement.md`.
A `Transform` on a node with a `ChildOf` parent would be a **second pose
owner**, and it would compose wrongly: `NodeVisualTransform` is the node's
*world* pose while a Bevy `Transform` is *local*.

So the collision pose is Avian's `Position`/`Rotation` alone. That has one
consequence to handle: Avian derives a collider's scale from the entity's
`Transform` (root bodies) or `ColliderTransform` (children) —
`update_collider_scale`, `avian3d-0.7.0/src/collision/collider/backend.rs:459` —
and a node with neither keeps collider scale 1. So the authored **scale is
folded into the shape** instead:

* `AffinePlacement::Trs` → `Cuboid(half × |scale|)` at the decomposed
  translation/rotation. Exact, because `Trs` is by construction the branch where
  the round trip lands back on the authored matrix;
* `AffinePlacement::Sheared` → `AffinePlacement::bake` of the box, which builds
  the exact parallelepiped, with the pose reduced to the translation.

Both halves are the one decision `crate::world::affine` already owns, reached
with a different split of the same matrix — not a second placement rule.

Measured on the pinned pair (`bevy 0.19.1` / `avian3d 0.7.0`): a collider on a
static body carrying only `Position`/`Rotation` (no `Transform`, no
`GlobalTransform`) still gets `ColliderOf { body: self }`, a correct broad-phase
`ColliderAabb`, and stops a probe. The `ColliderOf` relationship hook early-returns
without a `GlobalTransform`
(`avian3d-0.7.0/src/collision/collider/collider_hierarchy/mod.rs`), which skips
`RigidBodyColliders` bookkeeping — irrelevant for a static body, whose mass
properties are unused — and does not affect the collider's placement, which for a
body-owning collider is the body's own `Position`/`Rotation`.

### 3. The geometry is declared, never derived — and the mesh shape is absent

A `SceneNode` carries **no geometry**. Its `MeshBinding` is an *address* (a
stored mesh-array index plus a resolved catalog id, `cs_content/src/scene.rs:518`),
and the converted mesh behind that id (`cs_content::mesh::RenderMesh`) has no
extents, centre, radius or AABB record at all. So "with what shape" cannot be
answered from a node, and the honest move is to take the shape as an input and
report its absence:

* `NodeCollisionShape::Cuboid { half_extents_m }` — the same record
  `cs_content::world::WorldCollisionShape::Cuboid` carries, and placed the same
  way. A box is the one shape this engine already builds honestly from a
  *declared* primitive.
* `SceneCollisionGeometry` — the table, keyed by stable `SceneNodeId`, carried in
  a **one-shot resource** the load removes as it reads it. A map in the request's
  `Load` variant was tried first and made the whole enum 224 bytes, tripping
  `clippy::large_enum_variant` on a type whose `Unload` arm carries nothing; the
  resource is also the module's existing pattern (`AirframeDamageState` is a
  resource beside the request). The cost is stated in the type's docs: a refused
  load consumes the table, so a retry needs it again — and a load with no table
  builds no colliders and *reports every collider-role node by name*, so the
  failure mode is a visible gap, not a silent one.
* **The mesh-derived shape is deliberately not there.** `AffinePlacement::bake`'s
  tri-mesh branch carries its own warning — "Do not wire it to a `Collider`
  before F18-B's mesh policy and F18-D's census of sheared retail geometry have
  decided it" — and the same reasoning applies here. A hull or a bounding box for
  a part collider would be that undecided policy wearing this stage's clothes.
  So a node whose collision geometry should come from its mesh is **not
  buildable yet**, and says so.

The **presence record is inserted regardless** of whether a collider could be
built. That is the task's rule and it is the right one: the record is the
opt-in the pass queries on, and `ColliderPresenceReport::without_collider` is the
designed report for "the record is maintained, the engine has nothing to apply
it to". A node that gained a collider later is then already under the policy.

### The layer a node's collider is on: `Aircraft`, and it is a designed choice

`CollisionRole` never said which layer, and nothing in F11's spec, F18's finding
or the interaction matrix settles it. `CollisionLayer::Aircraft` is chosen: a
scene node is an airframe part, and the declared matrix
(`cs_sim::collision::CollisionLayer::designed_collides_with`) makes that interact
with aircraft, projectiles, debris and triggers. `StaticWorld` was the
alternative and was not chosen — "terrain, buildings and other immovable world
geometry" is not what a wing is, and it would have made an aircraft part stop
ordnance on a different row of the matrix than the rest of its airframe.

Also stated rather than hidden: the node is a **static** body at its composed
pose. **An airframe's collision does not fly with the airframe here.** Moving it
as one body is the flight/damage stages' decision, and nothing in this stage
moves a static body (`apply_force_requests` drops a force on anything that is not
dynamic, and the flight systems query `FlightAircraft`, which a scene node does
not carry). No other consumer in the crate queries `Collider` or `RigidBody`
outside `world/` and `physics/`, so the new body is contained.

## Tests

`crates/cs_app/tests/accept_f20_c_05_node_collider_spawn_wiring.rs`, prefix
`accept_f20_c_05_`. The world is the production composition: `headless_app`, the
adapter at the declared rate, `PhysicsBodiesPlugin`, `AnimationSchedulePlugin`
and `ColliderPresencePlugin` on a manual clock that runs one fixed step per
update, plus the F11-C scene systems in their required order. The scene is loaded
through `AirframeSceneRequest` served by `process_airframe_scene_request`, the
clip is bound through `bind_animated_node` (the producer the scene spawn path
calls), and every physics assertion is read from the production contact reporter.

| test | what it pins |
| --- | --- |
| `accept_f20_c_05_a_collider_node_is_loaded_with_its_presence_record_and_a_real_collider` | the wiring itself: `Live` **and** a real `Collider` on the node entity, no engine disable marker, the collider bound to that same entity (`ColliderOf.body == hatch`), a static `RigidBody`, the declared `Aircraft` layer, the collider's broad-phase box the declared 1 m box plus the engine's margin, and the load's `uncollidable` list empty. A `CollisionRole::None` node whose geometry **is** declared gets neither record nor collider, and a node whose collision role is an explicit unknown gets neither and keeps its unknown on its own `PartBinding`. Exactly one entity in the world carries the record |
| `accept_f20_c_05_a_clip_hidden_loaded_node_is_no_longer_an_obstacle` | the spawn-wiring version of the observation F20-C.04 makes for a body spawned directly, on the collision channel: a probe is stopped and reported while the clip shows the node, flies **through** the same volume with no contact reported against *any* entity of the live scene after the clip's authored hide tick (all three fixture nodes sit in that volume, so this is also the observation that the `None` and unknown nodes contributed no collider), and is stopped again after the show tick |
| `accept_f20_c_05_a_reload_releases_the_presence_record_with_the_node` | the third acceptance criterion: a reload consumes a fresh generation, the superseded node's entity is gone, **no** live entity of a superseded generation carries a `NodeColliderPresence`, the new node carries both records again, and a probe is stopped by the reloaded collider — a rebuild, not a wiring that stopped working |
| `accept_f20_c_05_a_collider_node_without_declared_geometry_is_reported_and_gets_no_collider` | the honesty half: a collider-role node with no declared geometry gets the opt-in, **no** collider, and a report naming the node and `UndeclaredGeometry` inside the load's own `Loaded` event; a clip hiding it moves the record to `HiddenByAnimation` and the pass reports `without_collider == 1` with `collider_writes == 0` rather than conjuring a collider |
| `accept_f20_c_05_the_geometry_table_refuses_a_degenerate_box_and_a_duplicate_node` | the declaration boundary: a zero or non-finite half extent is refused where it is declared, so is one that is finite in the record but infinite after the runtime's `f32` narrowing, and a node cannot be declared twice (`DuplicateNodeGeometry`) |
| `accept_f20_c_05_an_offset_node_is_placed_at_its_composed_pose_with_the_scale_in_the_shape` | **review addition**: where the collider is. A second fixture whose root is offset **and** turned, with a child carrying a canonical `z` scale of two: the collider body sits at the node's **composed** translation and carries its **composed** rotation, the declared box keeps the authored scale (`0.5 × 0.5 × 1.0` m), and the engine's own broad-phase box agrees |
| `accept_f20_c_05_a_sheared_node_carries_its_linear_map_in_the_shape` | **review addition**: the other placement. A child whose stored 3×3 carries a shear: the collider is the box's exact affine image (a convex polyhedron, not the declared box), the pose keeps only the authored translation with the identity rotation, and the engine's box is the sheared solid's 0.75 m vertical reach |

The first fixture's nodes all sit at the origin with an identity authored
transform, so they cannot tell a **composed** pose from an authored local one, a
dropped authored scale from a folded one, or a baked shear from an unbaked one.
Every part of a real airframe is an offset child, so the two placement tests
exist because that is where the wiring decides where colliders go; see
"Review findings" below for the probes that proved the gap.

## Mutation probes

Run with a rerunnable driver: each probe edited one production file
(`crates/cs_app/src/scene.rs`), ran
`cargo test -p cs_app --locked --test accept_f20_c_05_node_collider_spawn_wiring`,
recorded the exit code and the failing test names, and restored the file with
`git checkout --`. The tree was committed before the run, so a restore could
not discard work. The driver and the log were deleted afterwards;
`git status` shows no probe edit.

| probe | edit | tests that fail (of 5) |
| --- | --- | --- |
| P1 the wiring is not on the production load path | `opt_node_into_collision_policy` returns `None` immediately — the pre-task state, where the record had no writer | 4 (everything but the pure table test) |
| P2 the presence record is never inserted | the `NodeColliderPresence::Live` insert is removed | 4 |
| P3 the collider is never attached | `node_collider_bundle` is not called | 3 |
| P4 the authored role is ignored | every bound socket is opted in, whatever its collision role | 3 |
| P5 a shape is fabricated | a node with no declared geometry gets a default 0.5 m box | 1 (the report test) |
| P6 the gap is not reported | the returned gap is dropped instead of collected | 1 |
| P7 the collider is a child node | the collider bundle is spawned under `ChildOf(node)` instead of on the node | 3 |
| P8 the release is a no-op | `release_scene` despawns nothing | 1 (the reload test) |
| P9 control | no edit | 0 — the selection is green on the committed tree |

P7 is the one worth naming: a child collider still stops a probe (a standalone
collider is a first-class broad-phase citizen), so the *physics* observation
alone would not have caught the wrong layout. What catches it is the
`ColliderOf.body` assertion, which is the same-entity constraint F20-C.04
recorded. That is the constraint, pinned.

### Review probes (bunny-2, independent re-run)

The reviewer re-ran the driver on the submitted commit and added three probes
of the *placement*, because the first fixture sits at the origin and could not
speak to where a collider is built. On the submitted commit all three were
**invisible**:

| probe | edit | submitted (5 tests) | after the review additions (7 tests) |
| --- | --- | --- | --- |
| PA the collision pose is the node's **local** transform | `socket.pose()` → `node.local_transform()` | **0 failing** | 2 |
| PB the authored scale is dropped instead of folded into the shape | `Cuboid::new(half * scale.abs())` → `Cuboid::new(half)` | **0 failing** | 1 |
| PC a sheared placement skips the bake | the `Sheared` arm keeps the un-sheared box | **0 failing** | 1 |
| P1 re-run (control that the driver works) | the wiring removed | 4 | 4 |
| P4 re-run (the authored role) | every bound socket opted in | 3 | 3 |
| P7 re-run (the collider on a child) | the bundle spawned under `ChildOf(node)` | 3 | 3 |

So the submitted suite pinned **that** a node is wired and **that** a clip-hidden
one stops being an obstacle, and did not pin **where** the collider is. PA is
the dangerous one: every part of a real airframe is an offset child, so reading
the local transform would have put every part's collision one parent off — with
the whole suite green. The two placement tests above close that, and PA/PB/PC now
fail.

## Review findings

Three, all fixed on the branch:

1. **The placement was unpinned** (PA/PB/PC above). Fixed by the two placement
   tests: a second fixture whose root is offset *and* turned, with a
   rotation-times-scale child and a sheared child, asserting the composed pose,
   the scale inside the shape, the baked polyhedron and the engine's own
   broad-phase boxes. Nothing about the production decision changed — the probes
   found a hole in the tests, not a bug in the wiring, and the expected values
   in the new tests were hand-derived from the declared adapter (the first
   hand-derivation had the fixture's left-hand quarter turn backwards; the
   production pose was right).
2. **A declared extent that is finite in the record but infinite in `f32` was
   accepted.** `1e300` is a perfectly good `f64` and becomes an infinite half
   extent at the narrowing, which is a solid the broad phase cannot bound — and
   the refusal's own doc claimed a non-finite extent is refused.
   `NodeCollisionShape::cuboid` now makes the runtime's test as well as the
   record's, which is the same two-test rule `AffinePlacement::of` applies to
   its matrix, and the table test asserts it. (`cs_content::world`'s identical
   record is narrowed further away, in the world spawn path; that path is F18's
   and was not touched.)
3. **The absence of `CollisionEventsEnabled` on a node was unstated**, and it
   reads like an oversight next to F18's colliders, which carry it. It is
   deliberate: Avian emits one *directed* event per flagged collider, so a node
   that also flagged would double-report every contact into
   `ContactReports`' suppression counter. The partner is the moving body, and
   every layer whose contacts the reporter is meant to see is flagged by
   `spawn_body`. Stated in the module docs, with the "do not add it" warning.
   Two smaller doc corrections went with it: `SceneEvent::Loaded`'s `uncollidable`
   list is now described as the list of **bound** sockets (a socket whose
   gameplay role is an explicit unknown is never bound and is reported in
   `unresolved`), which is what the field actually contains.

Nothing else was changed. In particular the F11-C test change stands as
submitted: the report is a field inside `Loaded`, the three nodes are now
expected by name, and the other three F11-C tests pass untouched.

## Checks run

By the implementer, before handover:

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0.
- `cargo test --workspace --locked` — exit 0, 2548 tests passed, 0 failed.
- `cargo test --workspace --locked -- accept_f20_c_05_ --include-ignored` —
  exit 0, **5 tests matched**, all passing, none `#[ignore]`d.
- `cargo test --workspace --locked -- accept_f20 --include-ignored` — exit 0,
  **68 tests** across the F20 selection (13 `accept_f20_a_`, 10 `accept_f20_b_`,
  8 `accept_f20_c_01_`, 9 `accept_f20_c_02_`, 8 `accept_f20_c_03_`, 8
  `accept_f20_c_04_`, 5 `accept_f20_c_05_`, plus the F20 unit tests in the
  library), 0 failed, so no earlier F20 assertion moved. F20-C.04's own tests are
  unaffected by the spawn wiring, as intended: they spawn a body directly, so
  they still pin the policy without a scene load.
- the nine probes above.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p cs_app --no-deps` — this crate does
  not gate on rustdoc (89 pre-existing link warnings on `main`); this branch
  introduces none and leaves 87, so the count went **down**.

No command needed `CS_GAME_DIR`, and `CS_CAPABILITIES` was not exercised: this
stage reads no original data.

The branch was then rebased onto a newer `origin/main`, which brought in F20-C's
render-side draw consumer (`crates/cs_app/src/render/{sync,visibility}.rs` and
its finding). It touches no file this branch changes and no `Cargo.toml`/
`Cargo.lock`, and the rebase applied without a conflict — but the **full** four
checks were run again on the rebased tree rather than the lighter post-rebase
set, and the numbers above are from that run. F20-C.04's constraint is unchanged:
`apply_collider_presence` still runs after the animation advance, and the draw
consumer reads the composed verdict, so the two do not compete for a writer.

By the reviewer, on the branch with the three findings above fixed — the full
four checks, not the lighter post-rebase set, and this time with no rebase
involved:

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0.
- `cargo test --workspace --locked` — exit 0, 2550 passed, 0 failed (the two
  added tests).
- `cargo test --workspace --locked -- accept_f20_c_05_ --include-ignored` —
  exit 0, **7 matched**, all passing, none `#[ignore]`d.
- `cargo test --workspace --locked -- accept_f20 --include-ignored` — exit 0,
  70 passed, so no earlier F20 assertion moved.
- the six probes in "Review probes", plus one for the `f32` narrowing refusal
  (removing `survives_f32` fails the table test).
- `RUSTDOCFLAGS="-D warnings" cargo doc -p cs_app --no-deps` — this crate does
  not gate on rustdoc, and the review's three new intra-doc links added nothing:
  the error set is byte-identical with the review changes stashed (89 lines,
  diff empty).

## What one earlier test had to change, and why it is not a weakening

F11-C's `accept_f11_c_reload_releases_the_superseded_generation_and_leaves_no_old_root`
pins the whole log of a load, reload and release. Its fixture authors three
nodes as `CollisionRole::Collider` and declares no geometry for them, so each
load now reports them. The first draft reported that as an event of its own and
that broke four F11-C assertions — the log grew by one event per load, and
`AirframeSceneLog::last()` stopped being the load. The report is now a field
**inside** `SceneEvent::Loaded`, beside `unresolved`, which is where a gap in one
node's collision belongs: it is part of what the load did, and a model whose
collider nodes have no measured geometry yet would otherwise append one entry per
node to every load. The F11-C assertion was updated to expect the three nodes by
name; nothing was removed, and the other three F11-C tests pass unchanged.

## Unknowns

- **The original's node collision is unmeasured.** Which authored flags or
  partitions select a node's collision surface, what geometry a part's collider
  took, and which interaction layer it sat on are all unknown (the GameZ node
  array is undecoded, F13; `NodeBitFlagsCs` names every bit `UNK`). Everything
  here is a **designed** rule. **Affected content: every node whose original
  collision surface differs from its drawn geometry** — and F20-D's original-family
  gate is what may later claim otherwise.
- **The collision layer choice is designed.** `Aircraft` over `StaticWorld` is
  this stage's reading, argued in the source; nothing measures the original's.
  If it is wrong the symptom is a part that stops ordnance on the wrong matrix
  row, not a crash.
- **Whether an airframe's collision should move with the airframe is undecided**,
  and F23/F29's. A scene node's collider is a static body at its authored pose
  here.
- **Which nodes in a real model are `CollisionRole::Collider` is unmeasured** —
  that is F11-A's binding evidence, and the reason no production load declares
  geometry today.
- The **mesh-derived part collider** is not buildable, on purpose: F18-B's
  simplification policy is undecided, and `AffinePlacement::bake`'s tri-mesh
  branch says so in its own docs.

## Follow-ups

- **A producer for `SceneCollisionGeometry`.** The load consumes a table nobody
  fills yet, so no production scene gets a collider. Filling it means turning a
  node's mesh into a collision primitive, which is the mesh-policy decision above
  (F18-B/F18-D) and, for airframe parts, F29's. Until then every collider-role
  node is reported by name instead of silently passing shots — that is the state
  this stage can honestly reach.
- **`ColliderPresencePlugin` is not installed in any production composition.**
  `PhysicsSessionBuilder::configure` is the seam (as it is for
  `AnimationPlugin` and `FlightForcesPlugin`), and no crate-level session
  composes one yet, so the collision-presence pass runs only where a test or a
  future session installs it. Wiring it into a mission session is that session's
  task, not this one's; recorded here because the spawn wiring is inert without
  it.
- **F29's damage zones** must call `remove_collider_for_damage` /
  `restore_collider_after_repair` (already filed by F20-C.04). Now that a scene
  node can carry a managed collider, the seam has a production caller to have.
- The `.after(advance_animation_on_session_tick)` constraint on the presence pass
  is still unpinned by any test (F20-C.04's probe M4). The same-entity
  constraint it interacts with **is** pinned here (P7).

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F20-object-animation-and-authored-destruction-states.md` (`### F20-C`,
  behavior 3, AC01), `docs/contracts/FLIGHT-PHYSICS.md`.
- `docs/findings/2026-10-02-f20-c-04-collider-presence-consumer.md` (the
  consumer, the opt-in, the same-entity constraint and the cost this stage pays),
- `docs/findings/2026-09-30-f18-b-followup-sheared-world-object-placement.md`
  (`AffinePlacement`, the one placement decision, and the measured reason a
  collider's linear map must live in its shape),
- `docs/findings/2026-09-30-t424-collider-on-body-invariant.md` (the rule this
  layout satisfies),
- `docs/findings/2026-10-01-t401-trigger-volume-and-swept-ccd.md` (why a
  standalone collider is a broad-phase citizen, which is what P7 had to be
  caught by something other than a probe),
- `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md` and
  `docs/findings/2026-09-30-f11-c-part-socket-and-damage-visual-wiring.md` (the
  content records this stage reads, and the socket pose's single ownership),
- `crates/cs_app/src/physics/collider.rs` (read-only here),
  `crates/cs_app/src/scene.rs`, `crates/cs_app/src/world/{affine,spawn}.rs`
  (read-only here), `crates/cs_sim/src/collision.rs`,
- `avian3d 0.7.0`: `src/dynamics/ccd/mod.rs:526` (swept CCD's
  `Query<(&Collider, &ColliderOf)>`), `src/collision/collider/backend.rs:459`
  (`update_collider_scale` reads `Transform`/`ColliderTransform`),
  `src/collision/collider/collider_hierarchy/mod.rs` (`ColliderOf`'s hook and its
  `GlobalTransform` early return), `src/collider_tree/update.rs:124,196-204` (the
  add/remove observers, including removal on despawn), read from the pinned
  source under `~/.cargo/registry`.
