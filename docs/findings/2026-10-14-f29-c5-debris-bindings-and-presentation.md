# F29-C.5: publishing the authored debris bindings and presenting a wreck

Date: 2026-10-14. Task: F29-C.5 "Publish the authored debris bindings and
present spawned debris instances" (#669). Spec:
`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage `### F29-C`.
Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no render, no audio, so no
`private/evidence/` report is produced.

## Why this stage exists

F29-C.2 (#518) left two halves open on purpose, and the F29-C.2 finding filed
them as this task:

* the spawn path inserts no `PartDebrisBinding` — the record exists and the
  consumer converges on it, but no production path published one, exactly like
  `DamageZoneBinding` was unpublished then;
* `SpawnedDebris` carries no pose, mesh, velocity or collider, so nothing
  resolved its `source` to an object a player could eventually see.

This task is those two halves. It does not change the F29-C.2 pass: the
decision and the record stay where they were, and the tests that pin them still
pass unchanged.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/scene.rs`: the `PartDebrisPublish` resource (the
  beside-the-request, one-shot source the load takes), the publish loop inside
  `load_airframe_scene`, the new `SceneEvent::Loaded::unpublished_debris`
  field, and the module-doc paragraph naming the new step.
- `crates/cs_app/src/debris.rs`: the presentation stage — `DebrisLifetime`,
  `DebrisObject`, `DebrisObjects`, `DebrisWreck`, `DebrisPresentationRefusal`,
  `DebrisPresentationEvent`, `DebrisPresentationLog`, `DebrisPresentationReport`,
  `DebrisPresentationOutcome`, `present_debris`, the private `part_poses` and
  `wreck_collision_layers` helpers, and the module-doc bullet.
- `crates/cs_app/src/lib.rs` (doc only): the `debris` crate-doc paragraph names
  the presentation stage and the publish source instead of deferring them to
  "another stage".
- `crates/cs_app/tests/debris/publish_and_present.rs` (**new**): the 9
  `accept_f29_c5_` tests, compiled into the F29-C.2 debris binary by
  `tests/accept_f29_c_debris_spawn.rs` — a module, not a new target, for the
  CI runner's disk budget (the same reason the propulsion tests are modules of
  the damage-consumer binary).
- This file.

No protected path, no `cs_content` / `cs_sim` change, no original data, no
binary. One pre-existing exhaustive `assert_eq!` over
`SceneEvent::Loaded` literals in `scene.rs`'s own test module gained the new
field (`unpublished_debris: vec![]`); its strength is unchanged.

**One observable failure, before the change:** after the real load and the real
resolver destroyed `engine_1`, the part entity carried no
`PartDebrisBinding` — no production path published one — and the instance the
pass spawned stayed the bare record it was: the world held **0** bound part
entities and **0** wrecks where the designed rule requires the binding on the
part entity and, once the boundary resolves the object, one placed,
lifetime-bound, simulated wreck.

## The designed rule

### Publishing the bindings (scene load)

`PartDebrisPublish { actor, graph, objects }` is inserted by the session that
registered the actor, beside the `AirframeSceneRequest::Load` that imports the
airframe — the same one-shot, beside-the-request shape
`SceneCollisionGeometry` has, and taken on every path (a refused load included)
so a source can never outlive the load it was authored for. Per damage graph
node the graph names:

| node's own `scene_binding` | entry in `objects` | what the load does |
| --- | --- | --- |
| resolved, names an entity of this import | resolved object | inserts `PartDebrisBinding { actor, node, debris, generation }` on that part entity, stamped with the load's generation |
| resolved, names an entity of this import | `Resolved::Unknown` | inserts the binding with the unknown **verbatim**: #518's pass refuses it by name with its claim |
| resolved, names another root's subtree | any | skips silently — another airframe's part |
| unresolved, or absent | any | publishes nothing and names the node in `SceneEvent::Loaded::unpublished_debris` |
| any | no entry | skips silently: not an authored debris part |

The node → scene-node identity is the **damage graph's own `scene_binding`**,
never a second map this source could drift from. `objects` is the content
boundary's per-node debris resolution: a node with no entry is not an authored
debris part, because whether the original authored debris per part is
unmeasured (F29 "Research boundary") — the same "absence is not an error" rule
the consumer applies. The spawn path never guesses an object id.

### Presenting an instance (debris pass)

`present_debris(world, objects, now, lifetime)` is convergent, like the pass it
reads:

* one `DebrisWreck` per live `SpawnedDebris`, linked by entity rather than by a
  copy of its fields, carrying the actor, the destroyed part, the resolved
  object, `placed_at`, the lifetime and the generation;
* **the object is resolved, never guessed**: the instance's `source` is looked
  up in the content boundary's `DebrisObjects` table, and an id it does not
  know is refused by name (`UnresolvedObject`) — the weapon-effects consumer's
  rule for a sound the load did not deliver;
* **the placement is the destroyed part's own pose**, read through the
  published `PartDebrisBinding` from the part entity's `NodeVisualTransform` —
  the single visual pose owner (F11 non-negotiable behavior 4) — so a wreck
  cannot drift from the wreckage it fell off. The pose is bound the production
  body way (`spawn_body`): the Bevy `Transform` **and** Avian's own
  `Position`/`Rotation` pair all carry it, plus `TransformInterpolation` for
  the F23-C render easing. It is *not* bound as a bare `GlobalTransform`:
  Avian's body init reads `Position`/`Rotation` (falling back to `Transform`),
  and `RigidBody` requires a placeholder `Position` (which, under
  `PhysicsPlugins`, also requires an identity `Transform`), so a body spawned
  with only a `GlobalTransform` initializes at the world origin and the init
  hook overwrites the `GlobalTransform` with it (measured in the review
  through the real fixed loop; the
  `accept_f29_c5_the_wreck_simulates_at_the_parts_pose` test pins it). The
  part's scale is not inherited — the collider is the object's own radius and
  a sphere cannot carry an authored scale, the same scale-free placement
  `spawn_body` uses. A part entity with no pose is refused by name
  (`UnplacedDebris`); no wreck is placed at a guessed position;
* **the wreck is simulated, not a static prop**: an Avian `RigidBody::Dynamic`
  with the object's own collider radius, on the declared
  `CollisionLayer::Debris` (whose designed matrix already lets it meet
  aircraft, projectiles, world geometry, other debris and triggers), and
  **zero initial velocity** — an invented impulse would be a rule nobody
  measured;
* **the lifetime is the caller's value, in integer ticks**: a wreck placed at
  `placed_at` is presented at `placed_at .. placed_at + lifetime` and expires
  at the first pass at or after `placed_at + lifetime`. `DESIGNED_DEBRIS_LIFETIME`
  (600 ticks; five seconds at the designed 120 Hz fixed tick, F23-A) is a
  stated default, not a hidden one. How long the original left wreckage around
  is unmeasured, so the stage takes the number instead of asserting one;
* **the teardown is the convergence**: a wreck whose instance is gone — the
  state-driven half of the F29-C.2 teardown, `release_debris`, an aircraft swap,
  or a scene release that took the part entities with it — is despawned by the
  next pass, so a repair or a reload leaves no wreckage entity. A wreck that
  expired is named (`Expired`) rather than silently dropped, and a later pass
  presents a fresh one if the authority still calls the part destroyed: the
  lifetime bounds one placement, the authority decides whether a wreck exists.

**Designed behavior, not original data.** Whether the original spawned authored
debris when a damage part was destroyed, which object it used, where the
wreckage sat, how it moved, how long it lasted and whether a repair removed it
are **unmeasured** — F29's "Research boundary", and F29-D (#108) keeps the
original-family gate (it is blocked on opcode vocabulary and on an owner
ruling). Every rule above is this engine's designed behavior. **Affected
content:** every airframe whose scene a session loads *and* whose session
publishes a `PartDebrisPublish` source — today no production caller inserts
one, and the content boundary resolves no debris object, so **no retail
airframe changes behaviour yet**: the first production publisher decides what,
if anything, to bind. **Resolving tasks:** F29-D validates against original
data and requires `retail`.

## Tests

Every test drives production code — the production scene load, the production
declared → lowered path (`lower_graph` / `lower_policy`), the real
`DamageResolver`, `apply_debris_state` / `release_debris` and `present_debris` —
never a test-only bridge. Task test prefix: **`accept_f29_c5_`** (9 tests in
the F29-C.2 debris binary).

| test | what it pins |
| --- | --- |
| `accept_f29_c5_a_load_publishes_the_authored_debris_binding_on_each_part_entity` (minimum) | one binding per authored debris part with the resolved object verbatim, the explicit unknown verbatim and the load generation; nothing for a part the boundary says nothing about or a node the graph does not name; `Loaded` reports no gap |
| `accept_f29_c5_an_unresolved_scene_binding_is_reported_not_bound` | a part whose scene identity is an explicit unknown is named in `unpublished_debris`, bound nowhere |
| `accept_f29_c5_the_published_unknown_binding_is_refused_by_name_and_spawns_nothing` | the published unknown reaches the F29-C.2 pass intact: refused with its claim, nothing spawned, nothing presented |
| `accept_f29_c5_presenting_an_instance_gives_the_wreck_placement_lifetime_and_physics` | one wreck per instance: the part's own pose, the resolved object, the tick and lifetime, `RigidBody::Dynamic` + the object's collider + `LinearVelocity::ZERO` + the debris layer and its designed partners; a second pass is a no-op |
| `accept_f29_c5_an_unresolvable_source_and_an_unplaced_part_are_refused_by_name` | an id the table does not know and a part entity with no pose are refused by name; neither spawns a wreck |
| `accept_f29_c5_a_repair_and_a_reload_leave_no_wreckage_entity_and_no_dangling_binding` | the teardown the task names: repair → wreck Released; reload via `release_debris` → wreck Released; unload → every published binding gone; nothing left to present |
| `accept_f29_c5_a_wreck_expires_at_the_end_of_its_designed_lifetime` | presented through `placed_at + lifetime - 1`, expired at `placed_at + lifetime` (named `Expired`), re-presented on the next pass while the authority still calls the part destroyed |
| `accept_f29_c5_the_wreck_simulates_at_the_parts_pose` (review) | the placement is real physics: through the real `PhysicsPlugins::default()` fixed loop (`asset_stack::headless_app`, one manual tick, zero gravity) the wreck's Avian `Position` is the part's pose, not the world origin |
| `accept_f29_c5_the_designed_default_lifetime_bounds_a_wreck` | the stated default is 600 ticks and `DebrisWreck::expired` agrees at age 599 / 600 |

## Mutation probes

Each probe edited one production line, ran the task selection
(`-- accept_f29_c5_`) plus the F29-C.2 debris selection, recorded the failing
tests and restored the file from Git:

| probe | edit | result |
| --- | --- | --- |
| the publish loop never runs | `&publish` → `&None::<PartDebrisPublish>` in `load_airframe_scene` | **7 of 8 failed** — every test that reads a published binding or a wreck: `..._a_load_publishes_the_authored_debris_binding_on_each_part_entity`, `..._an_unresolved_scene_binding_is_reported_not_bound` (no bindings and no reported gap), `..._the_published_unknown_binding_is_refused_by_name_and_spawns_nothing`, `..._presenting_an_instance_gives_the_wreck_placement_lifetime_and_physics`, `..._an_unresolvable_source_and_an_unplaced_part_are_refused_by_name`, `..._a_repair_and_a_reload_leave_no_wreckage_entity_and_no_dangling_binding`, `..._a_wreck_expires_at_the_end_of_its_designed_lifetime`; only the pure lifetime-value test survived |
| the stage stops presenting | `objects.get(&debris.source)` → `objects.get(&debris.source).filter(|_| false)` in `present_debris` | 4 failed: `..._presenting_an_instance_gives_the_wreck_placement_lifetime_and_physics`, `..._an_unresolvable_source_and_an_unplaced_part_are_refused_by_name`, `..._a_repair_and_a_reload_leave_no_wreckage_entity_and_no_dangling_binding`, `..._a_wreck_expires_at_the_end_of_its_designed_lifetime` (0 wrecks, everything refused as `UnresolvedObject`) |
| the teardown stops releasing | `let instance_live = world.get_entity(wreck.instance).is_ok_and(...)` → `let instance_live = true;` in `present_debris` | 1 failed: `..._a_repair_and_a_reload_leave_no_wreckage_entity_and_no_dangling_binding` (the wreck survived the repair and the reload) |
| the placement is defaulted | `let Some(&pose) = poses.get(...)` → `poses.get(...).copied().unwrap_or(GlobalTransform::IDENTITY)` | 1 failed: `..._an_unresolvable_source_and_an_unplaced_part_are_refused_by_name` (the unplaced part was presented at identity instead of refused) |
| **review:** the placement is a bare `GlobalTransform` again | the wreck spawn's `Transform`/`Position`/`Rotation` triple → the original single `pose` (`GlobalTransform`) component | 2 failed: `..._the_wreck_simulates_at_the_parts_pose` — through the real fixed loop the wreck's Avian `Position` was `Vec3(0, 0, 0)` (the origin) while the part's pose was `Vec3(0, 0, -2)` — and `..._presenting_an_instance_gives_the_wreck_placement_lifetime_and_physics` (its `Transform` placement assertion: a bare-`GlobalTransform` wreck carries no placement `Transform`). The 7 other tests still passed, which is exactly how the bug survived the original suite: a bare `World` never runs Avian's init hook against the required-component graph the way `PhysicsPlugins::default()` does |

## Review fix (2026-10-14, bunny-alpha-1)

The reviewer found one real defect and fixed it: the wreck's placement was
spawned as a single bare `GlobalTransform` component. Avian's rigid-body
initialization never reads a `GlobalTransform` — `RigidBody` requires a
placeholder `Position` (which, under `PhysicsPlugins`, also requires an
identity `Transform`) — so the moment a real physics schedule ran, the body
initialized at the world origin and the init hook overwrote the spawned
`GlobalTransform` with it. The stage's central designed rule ("the placement
is the destroyed part's own pose") therefore held only for the bare-`World`
tests, never in the runtime this crate targets. Measured: part pose
`Vec3(0, 0, -2)`, simulated `Position(Vec3(0, 0, 0))` after one fixed tick.

The fix binds the pose the production `spawn_body` way: `Transform` (Bevy) and
`Position`/`Rotation` (Avian) all carry the part's translation/rotation, plus
`TransformInterpolation` for the F23-C render easing; the part's scale is not
inherited (the collider is the object's own radius; a sphere cannot carry an
authored scale — the same scale-free placement `spawn_body` uses). The new
test `accept_f29_c5_the_wreck_simulates_at_the_parts_pose` pins it through
`asset_stack::headless_app()` (real `PhysicsPlugins::default()`, one manual
fixed tick, zero gravity), and the placement assertion in
`..._presenting_an_instance_gives_the_wreck_placement_lifetime_and_physics`
now reads the `Transform`/`Position`/`Rotation` triple.

Reviewer identity: `bunny-alpha-1` (fresh review context, a different agent
instance from the implementer `bunny-alpha-2`); this review is independent of
the implementation session.

## Unknowns

- Whether the original authored debris per part, which object, how many
  instances, where the wreckage sat, its initial velocity, its lifetime and
  whether a repair or a restart removed it: **unmeasured**. `retail` file
  access measures damage *regions* and wreck *materials* (#521 / F29-D.1) but
  nothing that says a runtime spawn or presentation rule; F29-D (#108) is
  blocked pending opcode vocabulary and an owner ruling. Nothing in this stage
  claims an original behaviour.
- No production caller inserts `PartDebrisPublish` yet, and no production
  schedule calls `present_debris`: the seams are real and their tests drive
  them directly, the same staging the merged F29-C work has (filed as #668 for
  the schedule half).

## Follow-ups left open (filed with `create_tasks`)

- **Scheduling both halves beside the runtime damage tick — #668
  (F29-C-SCHEDULE).** Whoever owns the session's damage tick calls
  `apply_debris_state` / `release_debris` beside the other consumers, and
  `present_debris` beside them with the session's debris table and designed
  lifetime; the scene load's publish source is inserted by whoever owns the
  actor registration.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-C`,
  non-negotiable 1, "Research boundary"), `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-10-05-f29-c-2-debris-spawn-consumer.md` (the consumer
  this stage reads and the two follow-ups it filed).
- `crates/cs_app/src/weapon_effects.rs` (the presentation-consumer pattern:
  a record for the render path, not a rendering; refusals by name).
- `crates/cs_app/src/scene.rs` (the load's beside-the-request, one-shot source
  shape and the load's own transaction), `cs_sim::collision` (the declared
  collision layers and the designed matrix), F23-A (`BASELINE_FIXED_HZ`).
