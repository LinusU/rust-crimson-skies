# T401: a world trigger volume and Avian's swept CCD — the decision

Date: 2026-10-02. Task: #401 "Resolve the swept-CCD vs sensor-volume interaction
for world trigger volumes", filed by the F18-A review (#82) and dependent on
F18-B (#86). Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read,
no evidence report required, nothing `verified_original`.

Pinned pair: `bevy 0.19.1` / `avian3d 0.7.0` / `parry3d 0.27.0`,
`SubstepCount(1)`, 120 Hz fixed, gravity zero. Probe: the production
`spawn_swept_probe` body (0.5 m box, `SweptCcd` + `SpeculativeMargin::ZERO`) on
the `Aircraft` layer. 400 m/s is 3.33 m per tick; 30 m/s is 0.25 m per tick; 60
m/s is 0.5 m per tick.

## The defect this task inherited, and why the role says it cannot be right

F18-A measured the interaction and filed it here: with a body on the trigger
volume, a 400 m/s probe crossing the 8 m `trigger.sensor` of the arch world ended
at `x = 19.0835` instead of `21.5`, a loss of 2.416 m — exactly the distance from
its previous sample (`x = 4.8333`) to the volume's near face including the
probe's half extent. Reading `avian3d-0.7.0/src/dynamics/ccd/mod.rs::solve_swept_ccd`
explains it: the sweep stops a body at the first time of impact against **any**
collider its path reaches, with no `Sensor` filter anywhere in
`compute_ccd_toi`.

What made it a *defect* rather than an engine fact is that the workspace had
already declared the opposite. `cs_sim::collision::classify_contact` makes every
sensor pair a `ContactKind::SensorOverlap` and never a `SolidContact`
(`crates/cs_sim/src/collision.rs`), and `WorldCollisionRole::Sensor` is documented
as "reports an overlap, never blocks motion". On the pinned pair the physics
contradicted the contract the records were written against, and every mission
built on it would have had its aircraft stop dead at its own checkpoints.

## Why the layout, and not a filter, is the lever

The sweep and the narrow phase consume the **same pair set**, so anything that
hides a collider from one hides it from both. Measured consequences, all on the
pinned pair:

* **Collision layers cannot do it.** The pair is created by the broad phase,
  which filters by `CollisionLayers` membership and filter
  (`collision/broad_phase/bvh_broad_phase.rs::collect_collision_pairs`). A
  trigger volume the aircraft does not interact with is never a candidate for
  either, so the crossing is never reported and every F18-C overlay bound to it
  silently stops firing. Rejected: it deletes the thing the volume exists for.
* **A `SweepMode` or an `include_dynamic` setting cannot do it.** The only
  skip in `solve_swept_ccd` besides the failed body query is
  `if !ccd1.include_dynamic && body2.rb.is_dynamic() { continue }`, and it tests
  the *other* body against the swept body's own setting. A trigger volume spawned
  `RigidBody::Static` is not dynamic, so the pair is still resolved and the body
  is still held. Measured, not assumed: the "body-bearing" test below is exactly
  this case and it holds the body.
* **A collider on a child node cannot do it, and must not be used to.** Task
  #420 measured that a child-node collider is invisible to the sweep, and task
  #424 turned "a body that swept bodies must stop against carries a `Collider`
  on its own entity" into an enforced invariant. Hiding a trigger volume by
  giving it a body with no collider would declare itself swept-invisible to
  exploit an audit that exists to catch regressions. Rejected on the same
  grounds T420 rejected the approach for real geometry.
* **What is left is the collider's body binding, and it is enough.**
  `solve_swept_ccd` iterates `contact_graph.entities_colliding_with(entity)` and
  resolves each candidate through `colliders: Query<(&Collider, &ColliderOf)>`
  (`dynamics/ccd/mod.rs`, lines 526 and 568) before it ever looks at the shape.
  `ColliderOf` is inserted by `ColliderHierarchyPlugin` **only** for a collider on
  a rigid body or below one
  (`collision/collider/collider_hierarchy/plugin.rs:14-40`), so a collider with no
  rigid body anywhere above it is never a candidate. The skip happens before any
  shape cast: no time of impact, no clamp, no `Sensor` filter needed.

So the decided configuration is:

> **A `WorldCollisionRole::Sensor` object is spawned on an entity that carries no
> `RigidBody` at all.** A standalone collider.

Avian supports that layout as a first-class citizen rather than a tolerated edge
case, which is what makes it safe to build on:

* the broad phase gives a body-less collider its own tree
  (`ColliderTreeType::Standalone`, `collider_tree/update.rs::add_to_tree_on`, and
  the "Query standalone tree (colliders with no body)" arm in
  `bvh_broad_phase.rs`);
* `add_to_tree_on` fires on `Insert, (Collider, ColliderOf)` — on insertion of
  *either*, so a collider that never gets a `ColliderOf` is still inserted, into
  the standalone tree;
* `CollisionStart` carries `body1`/`body2` as `Option<Entity>` precisely so a
  body-less collider can be one side of a pair
  (`collision/collision_events.rs:179-186`), and the narrow phase writes the
  event for any pair that starts touching
  (`collision/narrow_phase/system_param.rs`, the `collision_started()` arm);
* a sensor's contacts are kept out of the constraint graph by
  `on_add_sensor`, and a pair with a `None` body generates no manifold anyway, so
  nothing can push a body.

## Measured behaviour on the pinned pair

Every number below was produced by running the production composition
(`cs_app::world::fixture::world_app` + the F23-A fixed-rate adapter +
`spawn_world`/`spawn_swept_probe`) and is asserted by a committed test.

### 1. The cuboid trigger volume (the arch world's `trigger.sensor`, 8 m along `x`)

400 m/s, 15 ticks, 50 m of travel from `x = -28.5`:

| | before | after |
| --- | --- | --- |
| every tick | 3.3333 m **except the crossing tick** | 3.3333 m, all fifteen |
| total distance | 48.58 m (2.416 m lost) | 50.00 m, drift < 0.01 m |
| velocity at the end | 400 m/s | 400 m/s |
| crossings reported | 1 | 1, with `role == Sensor` and the probe named |

Tick positions after the change, unchanged from a free flight:
`-25.17, -21.83, -18.50, -15.17, -11.83, -8.50, -5.17, -1.83, 1.50, 4.83, 8.17,
11.50, 14.83, 18.17, 21.50`. The contact is logged on the **twelfth** of those
samples (`x = 11.50`, inside the volume at `x ∈ [6, 14]`) and stays at one entry
— reported **once**, which is F23's AC02 property as well as F18's role claim.
(The log turns up one step after the first sample that is itself inside the
volume, `x = 8.17`. That one-step lag is measured, not explained: the report is a
`CollisionStart` on the pair the broad phase created, and this record does not
claim to know why that pair appears a step after the geometry it is built from.
What the test pins is the count, and it is one.)

Pinned by
`trigger::accept_f18_b_a_swept_body_crosses_a_world_trigger_volume_untouched`.

### 2. The mesh trigger volume (the harbor world's `trigger.sensor`, 8 m along `x`, 12 stored triangles)

| speed | tick | travel | crossing reported |
| --- | --- | --- | --- |
| 30 m/s | 0.25 m | free, all 150 ticks | **yes, once** |
| 400 m/s | 3.33 m | free, all 15 ticks | **no** |

Both halves are asserted in
`trigger::accept_f18_b_a_mesh_trigger_volume_reports_a_swept_body_where_a_sample_lands`.
The second row is a limitation, not a property, and is treated as one below.

### 3. The layout that was rejected, measured

One `RigidBody::Static` put back on the mesh trigger volume — all it takes to
restore the old behaviour, through the same production spawn — and the same
400 m/s flight (15 ticks from `x = -28.5`):

* per-tick travel over the fourteen steps between the fifteen samples
  `[3.333 ×8, 0.887, 0.030, 3.333, 3.333, 0.584, 0.750]`: the body is **held
  across four of those steps** (the test counts a step as held when it is under
  half a free tick) and loses **11.08 m**, ending at `x = 13.75` instead of
  `21.5`;
* the crossing **is** reported — once. Holding the body at the volume's surface
  is exactly what gives the narrow phase something to see, so the clamp bought a
  report at this speed. That is the trade, and it is the whole of it: eleven
  metres of a 50 m flight, bought with a report the report path cannot produce on
  its own at this speed.

Pinned by
`trigger::accept_f18_b_a_body_bearing_trigger_volume_holds_a_swept_body_in_four_ticks`,
which is also the re-measure signal: if a future avian release teaches the sweep
to skip sensors, that test fails, the body can go back, and the test inverts.

### 4. The layout is the record's role and nothing else

A `Solid` object is spawned on `RigidBody::Static` with the derived collider bound
to it (`ColliderOf` present) and is reported with `body: Some(entity)`; a
`Sensor` object has no `RigidBody`, no `ColliderOf`, `body: None`, and
`SpawnedObject::entities()` lists the entities that exist. Pinned by
`trigger::accept_f18_b_the_layout_is_the_recorded_role_and_nothing_else`.

On the mesh path the trigger bundle is written out separately from the
collider-on-body one, so nothing but the body and the `Sensor` marker may differ:
same `Mesh3d` handle resolving in the world's own asset stack, same stored
triangles (12 for the box, 36 for the hangar shell — no hull, no bounding box),
same `Position` as `Transform`, same layers, same binding. Pinned by
`trigger::accept_f18_b_the_trigger_and_solid_mesh_paths_differ_only_in_the_body`.

### 5. Task #424's invariant is unaffected

`cs_app::asset_stack::swept_invisible_bodies` and
`undeclared_swept_invisible_bodies` are both empty after a harbor world loads
with the new layout: their query is `With<RigidBody>`, and a trigger volume is
not a rigid body, so it is outside the query rather than exempted from it. The
decision does not weaken the collider-on-body rule; it sits beside it. Pinned by
`trigger::accept_f18_b_the_swept_visible_body_audit_still_holds`, and by the
existing `accept_t424_the_world_import_path_leaves_no_body_invisible_to_a_sweep`,
whose world-import arm was updated for the `Option` (its trigger-volume arm now
asserts the *absence* of a body instead of the presence of one).

### 6. F18-C: the overlay still fires

`overlays::accept_f18_c_a_swept_body_crosses_a_trigger_volume_and_the_overlay_still_fires`,
on the depot flight (1 m trigger volume, door 4 m ahead):

* at 60 m/s (0.5 m per tick) the swept body crosses the volume, the contact
  stream names `trigger.depot`, the consumer applies the overlay **once**, and
  both halves of the door move by the record's offset;
* at 400 m/s (3.33 m per tick) **no tick that carries the body across the volume
  is short**. The body is then stopped by the **closed door** on the next tick,
  because at that speed the overlay does not fire (see the limitation below), and
  the test says so rather than reading that stop as a hold.

The F18-C test that used to assert the clamp (`..._pays_for_the_sensor_face`, and
it said so in its own comment: *"if #401's fix lands, the clamp assertion fails,
and that failure is the signal to re-measure"*) is replaced by the above.

## Alternatives measured and rejected

* **A collision layer the swept body does not interact with.** Rejected above and
  by measurement: the crossing is then never reported, so no overlay fires. It is
  the layer route F39 might want for volumes that should be invisible, but it
  cannot be how a trigger volume is spawned.
* **A `SpeculativeMargin` on the volume.** Measured: `SpeculativeMargin(4.0)` on
  the mesh trigger volume, with the production probe's own `SpeculativeMargin::ZERO`
  in place, still reports **nothing** at 400 m/s. The pair's effective margin is
  `dt · |v₂ − v₁|` *after* each side's velocity is clamped to its own
  `margin / dt` (`collision/narrow_phase/system_param.rs:646-689`), so zeroing
  the margin on the fast body collapses the pair's to zero no matter what the
  volume asks for. A nonzero margin on the **body** would widen it — and that is
  the globally inflated hitbox F23 non-negotiable behavior 3 forbids, measured and
  rejected by task #420 at 1 m and 2 m thresholds.
* **A `CollisionMargin` on the volume.** Measured: `CollisionMargin(2.0)` on the
  mesh trigger volume makes the 400 m/s crossing reported again, with the body
  still crossing free. It works, and it is **not adopted**, because 2 m is a
  tolerance this task has no evidence for: it decides how close to an authored
  volume a body must be before the volume claims it, which is a gameplay rule
  about triggers (F39) and not a fact about the engine. Recorded here as a
  measured option with its cost, not as a default.
* **A `SweptCcd` on the volume's own body.** Meaningless: `solve_swept_ccd` only
  sweeps entities that carry `SweptCcd`, and the volume is the target, not the
  projectile.
* **Substeps.** Not applicable to the decision (the pair is skipped before the
  solver subdivides), and T420 measured substeps do not help the collider-side
  question either.
* **A pin bump or a `[patch]`.** `avian3d 0.7.0` is the newest release; upstream
  `main` has rewritten swept CCD to iterate `RigidBodyColliders` and so will not
  need this layout, but that is unreleased. The pinned tests fail loudly when a
  release containing it lands.

## The limitation this decision leaves, and what resolves it

**A trigger volume's report is a discrete overlap, so a body whose tick outruns
the volume's thickness is not reported of it.**

Mechanism, from the source and confirmed by measurement: the narrow phase asks
parry for a manifold within `max_contact_distance`, which is
`max(dt · |v₂ − v₁| after clamping, contact_tolerance) + collision_margin_sum`.
A swept body on a swept layer carries `SpeculativeMargin::ZERO`, so the effective
margin collapses to the contact tolerance. A **triangle mesh** only produces
contacts near a triangle, so a body that lands *deep inside* a mesh volume with
no triangle within reach is never reported — measured, and pinned by the fast arm
of
`trigger::accept_f18_b_a_mesh_trigger_volume_reports_a_swept_body_where_a_sample_lands`.
A **cuboid** pair has no such gap: parry's EPA produces a deep-penetration
contact, which is why the arch world's volume is reported at 400 m/s.

* Not caused by this decision: a body *parked* inside such a volume was never
  reported either. What the clamp did was force a moving body onto the surface,
  which looked like a report.
* Affects: **any** trigger or objective volume whose thickness is under one tick
  of travel at the reaching body's speed, mesh-derived or not. At 120 Hz that is
  3.33 m of thickness at 400 m/s and 1.6 m at a dive's 194 m/s, so a thin
  authored volume can be missed by a fast aircraft. **Measured on F18-C's own
  depot volume**, which is a *cuboid*: at 30 m/s and 60 m/s (0.25 m and 0.5 m of
  travel against its 1 m thickness) the contact stream names `trigger.depot`; at
  400 m/s (3.33 m) it names only `depot.door` — no sample of that flight lands
  inside the volume, cuboid or not, and parry's deep-penetration contact has
  nothing to be deep about. (Re-measured during the #401 review; an earlier
  draft of this record claimed the depot volume was unaffected because it is a
  cuboid, which its own §6 and this measurement both contradict.)
* Does not affect: any volume **thicker than one tick** of travel at the
  reaching body's speed, whatever its shape — which includes every mission-sized
  volume the fixtures author. A mesh-derived volume has that one boundary and a
  second, separate one: the deep-inside gap above, which a cuboid does not have.
* Resolving task: **#498** (`F18-trigger-swept-crossing`), filed by this task —
  a **swept crossing report** for trigger volumes, the crossing decided from the
  body's own motion over the tick rather than from a sampled overlap. F39 owns
  trigger semantics ("objectives, triggers, timers, spawn groups"); F23's AC02
  ("high-speed crossing of a thin wall/trigger is detected exactly once") is the
  same question from the body side. **#498 is not the same task as #415**
  ("decide and implement consumption of the spawn-tick trigger crossing record",
  filed by the F23-D review), which covers the same question at the *spawn* tick
  and already records a non-blocking `SpawnPreflightEvent::passed` there. The two
  are the same boundary at two entry points: ordinary flight and the spawn tick.
  Whoever takes either should read the other, so the crossing is decided once and
  reported the same way in both places. Until #498 lands, no claim is made that a
  swept body crossing a **mesh-derived** trigger volume — or one thinner than a
  tick — always fires its overlay.

## Known limitations that gate later stages (not silently dropped)

* The limitation above: the discrete report boundary of a trigger volume — a
  volume thinner than one tick of the reaching body's travel is not reported at
  all, and a *mesh-derived* one additionally goes quiet when a body lands deep
  inside it — with the affected content, the measured depot case and the
  resolving task named.
* **`SpawnedCollider::body` is now `Option<Entity>`.** A consumer that stamps,
  moves or despawns *by body entity* must handle `None`. The only body-less
  objects are `Sensor` ones, and the load transaction despawns
  `SpawnedObject::entities()`, which lists the entities that exist, so the
  residency path needed no change. Any future consumer that assumed a body is
  handed a report that refuses to pretend.
* **The mesh trigger bundle is written out twice.** `spawn_mesh_trigger_volume` in
  `crates/cs_app/src/world/spawn.rs` is
  `asset_stack::spawn_static_mesh_collider_on_body` minus the rigid body, and it
  lives here because that helper is F00-A's path and it *is* the layout a trigger
  volume must not use. The duplication is held to the solid path by
  `accept_f18_b_the_trigger_and_solid_mesh_paths_differ_only_in_the_body`; if
  `asset_stack`'s bundle ever grows a component, that test is the thing that says
  so.
* **Nothing here is `verified_original`.** Whether the 2000 PC original used
  trigger volumes as sensors, as solid volumes the game tested differently, or
  not at all is unmeasured, and no claim is made about it. The role vocabulary
  (`WorldCollisionRole::Sensor`) stays designed project content; retail per-object
  role assignment remains F18-B/D's evidence question.
* **The tunnel-mouth artifact is not this task's, and is recorded so nobody
  mistakes it for one.** A swept body through the fixture *mesh* arch loses
  0.15 m of one tick at the mouth of the opening (measured: the identical
  0.350 m step at 60 m/s through the harbor world's arch, with no sensor in the
  flight, and the same shape at 400 m/s), and the body then creeps along a
  zero-thickness face when something stops it — the behaviour task #420 measured
  against a wall. F18-C's crossing assertion is scoped to the ticks that carry
  the body across the volume for exactly this reason.

## Test sensitivity (mutation matrix)

Every mutation below was applied, the whole `crates/cs_app/tests/world` binary was
run (`cargo test -p cs_app --test world` — a `accept_f18_b_` filter would hide the
`accept_f18_a_` and `accept_f18_c_` failures the first four rows report), and the
source was restored. All 38 tests under the `accept_f18_b_` prefix pass
unmutated (32 from F18-B/#421, 6 from this task).

| mutation | tests that failed |
| --- | --- |
| the cuboid sensor gets a `RigidBody::Static` back | `spawn::..._every_collision_role_decides_what_is_spawned`, `trigger::..._the_layout_is_the_recorded_role_and_nothing_else`, `trigger::..._a_swept_body_crosses_a_world_trigger_volume_untouched`, `overlays::..._a_swept_body_crosses_a_trigger_volume_and_the_overlay_still_fires` (4) |
| the mesh sensor goes back to `spawn_static_mesh_collider_on_body` | `trigger::..._a_body_bearing_trigger_volume_holds_a_swept_body_in_four_ticks`, `trigger::..._the_trigger_and_solid_mesh_paths_differ_only_in_the_body`, `trigger::..._the_swept_visible_body_audit_still_holds`, `trigger::..._a_mesh_trigger_volume_reports_a_swept_body_where_a_sample_lands`, `import::..._a_mesh_role_solid_stops_a_body_and_sensor_only_reports_one` (5) |
| the report always names a body (`body: Some(entity)`) | `spawn::..._every_collision_role_decides_what_is_spawned`, `trigger::..._the_layout_is_the_recorded_role_and_nothing_else` (2) |
| the cuboid sensor loses the Avian `Sensor` marker | `shear::..._a_sheared_object_still_follows_its_declared_role`, `spawn::..._every_collision_role_decides_what_is_spawned`, `trigger::..._the_layout_is_the_recorded_role_and_nothing_else` (3) |
| the mesh sensor loses the Avian `Sensor` marker | `trigger::..._the_trigger_and_solid_mesh_paths_differ_only_in_the_body` (1) |

The #401 review re-applied all five and confirmed every row above, the counts
included. (The second row listed four; it is five — the F18-B import test that
holds a mesh `Sensor` role against a solid one fails as well, and should have
been in the list.)

The fourth row is worth reading rather than skipping: **the crossing test does not
catch a missing `Sensor` marker**, and neither does the mesh report test. A
body-less collider is not resolved by the solver in any case — the narrow phase
sets `GENERATE_CONSTRAINTS` off for any pair with a `None` body *before* it looks
at either collider (`collision/narrow_phase/system_param.rs`, `is_disabled`), so
dropping the marker changes what the volume *means* to Avian without changing
what it does to a body. The marker is pinned by the role tests, which is where its
meaning is; the crossing tests are about motion, and say nothing about it.

That is a statement about the *body-less* layout, not a licence to spawn a sensor
without the marker. A body-*bearing* collider that loses the marker is a solid
wall: both sides of the pair then have a body and neither is a sensor, so
`is_disabled` is false, constraints are generated, and the solver stops the body
where the volume is. Measured during the #401 review by putting the body back
*and* dropping the marker together: the same 400 m/s flight ends at `x = 5.66`
after being held at `x = 5.75` and then pushed out along `y` and `z`. Row 1 and
row 4 are the same experiment from opposite ends, and the marker is load-bearing
in both.

## Evidence

Ordinary build/test only; no `CS_GAME_DIR` read and no evidence report is
required for this task. Commands run locally:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # exit 0
cargo test --workspace --locked -- accept_f18_b_ --include-ignored
#   38 tests run, 38 passed (crates/cs_app/tests/world)
```

**Review (#401, `bunny-2`, fresh context).** Every number in §1–§6 was
re-measured through the same production composition, all five mutations were
re-applied and reverted, and the four checks plus the three task selections were
re-run on the rebased tree:

```sh
cargo test --workspace --locked -- accept_f18_b_ --include-ignored   # 38 run, 38 passed
cargo test --workspace --locked -- accept_f18_c_ --include-ignored   # 18 run, 18 passed
cargo test --workspace --locked -- accept_t424_ --include-ignored    # 12 run, 12 passed
```

Reproduced exactly: the arch flight's fifteen samples and its 3.8 µm of drift;
the mesh volume reported once at 30 m/s and not at all at 400 m/s; the rejected
layout's `[3.333 ×8, 0.887, 0.030, 3.333, 3.333, 0.584, 0.750]`, its 11.08 m loss
and its single report; and the mutation rows above, with one correction (the
second row listed four failures, not five). The one measurement the review added
is the depot's own 1 m **cuboid** trigger volume at 30, 60 and 400 m/s, which
corrected the "does not affect" line in the limitation section above. No original
data was read and nothing here is `verified_original`.

The six acceptance tests this task adds, all in
`crates/cs_app/tests/world/trigger.rs`:

* `accept_f18_b_a_swept_body_crosses_a_world_trigger_volume_untouched` — the
  decision on the volume F18-A measured it on: free travel on every tick,
  unchanged velocity, reported exactly once.
* `accept_f18_b_a_mesh_trigger_volume_reports_a_swept_body_where_a_sample_lands`
  — the import path, and the report boundary as a pinned limitation.
* `accept_f18_b_a_body_bearing_trigger_volume_holds_a_swept_body_in_four_ticks`
  — the rejected layout, measured, including what it bought.
* `accept_f18_b_the_layout_is_the_recorded_role_and_nothing_else` — the decision
  is derived from the record's role and reports itself honestly.
* `accept_f18_b_the_trigger_and_solid_mesh_paths_differ_only_in_the_body` — the
  two mesh layouts held against each other.
* `accept_f18_b_the_swept_visible_body_audit_still_holds` — #424's invariant,
  unaffected.

## Sources

Pinned `avian3d-0.7.0` and `parry3d-0.27.0` sources in the local cargo registry
(`dynamics/ccd/mod.rs`, `collision/collider/collider_hierarchy/plugin.rs`,
`collision/collider/collider_transform/plugin.rs`, `collider_tree/update.rs`,
`collision/broad_phase/bvh_broad_phase.rs`, `collision/narrow_phase/system_param.rs`,
`collision/collision_events.rs`), each read and then **measured** through the
production composition rather than trusted. The stage records this decision
belongs to: `docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`
(which filed the limitation), `…-f18-b-world-import-and-static-collision.md`,
`…-f18-c-mission-overlays-and-visibility-streaming.md`,
`docs/findings/2026-09-30-t420-mesh-ccd-decision.md` and
`…-t424-collider-on-body-invariant.md` (the collider-on-body rule this decision
sits beside), plus `cs_sim::collision`'s `classify_contact`, which is the contract
the engine was contradicting. No original data was read and no original behavior
is claimed.
