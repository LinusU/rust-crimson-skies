# F11-C: part sockets, damage visuals and scene load/unload lifetime

Date: 2026-09-30. Task: F11-C "Bind aircraft parts, sockets and damage visuals"
(`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, section
`### F11-C`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Required
capability: ordinary build/test only — **no** `retail`, `gpu` or `audio` was
used: this stage reads no original data, renders nothing and plays nothing;
every fixture value is newly authored.

The acceptance scenario for this stage (AC03) is *load and unload the same
airframe 100 times without increasing live entity count*.

## Scope decision (taken before editing)

This stage wires what F11-A/B built into the running app: it serves one
airframe load/unload request, binds the content layer's semantic sockets onto
the imported entities, reflects recorded damage onto the presentation
markers, and owns the teardown/retry and error paths. It does **not** add a
format reader, a camera, a renderer mapping or gameplay damage rules; those
belong to F11-D/F15/F17/F21/F29 and are named under Unknowns.

The GameZ **node-array byte reader is still not part of any F11 stage** (see
F11-B's findings). `crates/cs_formats/` stops at
`GameZHeader::nodes_offset`; no F11 stage owns that crate. So the request
resource carries an **already converted** `SceneGraph`; the container bytes
cannot be read here and no layout was guessed. The recorded blocker and its
task (**#392**) stay in force.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/scene.rs` (extend): `PartSocket` (node, `Resolved`
  role and collision role, composed pose, zone id, animation bindings,
  provenance) and the `SceneGraph` socket table with `sockets()`,
  `socket(node)`, `sockets_of_role(role)` and `unresolved_sockets()`.
  `SceneGraph` also gained `PartialEq` so an app-side request resource can
  derive it.
- `crates/cs_app/src/scene.rs` (extend): `SceneImport::entities()` (the
  teardown list) and the whole F11-C wiring — `AirframeSceneRequest`,
  `SceneGenerations`, `LiveAirframeScene`, `PartBinding`, `SceneEvent`,
  `AirframeSceneLog`, `AirframeDamageState`, the exclusive systems
  `process_airframe_scene_request` and `apply_airframe_damage`, and the
  `accept_f11_c_*` tests.
- `crates/cs_content/tests/scene.rs` (extend): the `accept_f11_c_*` test of
  the socket rule.
- Wiring only (no logic): a `crates/cs_app/src/lib.rs` doc paragraph naming
  the F11-C records (AGENTS rule 1 wiring exception).

**One observable failure:** if the unload path despawned nothing (or only the
root's direct children), then
`accept_f11_c_load_and_unload_the_same_airframe_hundred_times_without_leaking_entities`
fails on the first round with `the unload released every node of the scene:
left: 13, right: 0`, and the world's total entity count would grow by the
subtree size on every one of the hundred rounds.

## Design decisions

- **A request resource is the producer → consumer hand-off.**
  `AirframeSceneRequest` is inserted by the producer and consumed *once* by
  the exclusive system `process_airframe_scene_request`
  (`world.remove_resource`). A request is never a stored wish that survives
  into a later run, and it is never applied twice. It has two arms: `Load {
  airframe, graph }` and `Unload`. The graph is an `Arc<SceneGraph>` so the
  request shares one converted container instead of copying it; the live
  record keeps that `Arc` (the mesh associations and animation channels a
  renderer or an animator needs live on the content node) and drops it on
  unload.
- **A load is prepare-then-commit, so a refusal cannot cost a model.**
  The load takes the next generation, imports the referenced subtree
  (`import_airframe` converts every transform before it spawns, F11-B), binds
  the sockets, releases the scene it supersedes and only then publishes the
  new `LiveAirframeScene`. A refusal (`ForeignContainer`, `UnknownNode`,
  `Transform`) spawns nothing, leaves the running scene's entities and record
  untouched, and is reported as `SceneEvent::Refused` with the error verbatim.
  The refusal is the retry point: the caller fixes the request and sends it
  again.
- **Generations are consumed, never reused.** `SceneGenerations` is a
  monotonic counter resource that outlives unloads, and `take_next` is called
  *before* the attempt, so a refused load burns its stamp and the retry is
  distinguishable (F11 non-negotiable behavior 5). Every imported entity is
  stamped with it, so a stale binding is detectable by generation mismatch
  rather than by a surviving pointer.
- **Teardown releases exactly the live record's own entities.**
  `release_scene` walks `SceneImport::entities()` (stable ids, not positions),
  counts how many are still present — Bevy takes a node's descendants with it,
  so a child an earlier iteration already took is simply gone — and despawns
  them. A release can therefore never take another generation's entities, and
  an entity some other stage already despawned is not counted.
- **A reload releases the superseded generation; an unload with nothing live
  is a no-op.** Both are reported as `SceneEvent::Released` with the
  generation and the count. Repeated teardown, and teardown after a refused
  load, are safe and report nothing.
- **Sockets bind by identity, and only when the role is known.**
  `cs_content::scene::PartSocket` is the evidence-backed record (F11-C's
  "semantic sockets"); the loader inserts a `PartBinding` (role, collision
  role, zone id, rule provenance) on the node entity for each socket inside
  the imported subtree. A socket on another root of the same container is
  another airframe's part and is not touched. A socket whose role is
  `Resolved::Unknown` is **not** bound and is listed in the load report's
  `unresolved` field — the gap is visible, never defaulted to a role.
  `PartBinding` deliberately carries no transform: the node entity's
  `NodeVisualTransform` is the single pose owner and it is the same value
  collision evaluates, so a weapon origin cannot drift from the visual (F11
  non-negotiable behavior 4; `IDENTITY-CONTENT`: one pose owner). The socket's
  `pose` is a *copy of that same composed transform* for consumers that read
  the content record directly, which is why the content test asserts it
  against `world_transform`/`visual_transform`/`collision_transform`.
- **The damage state is the single owner of `NodeDisabled`.**
  `AirframeDamageState` records destroyed parts as stable `SceneNodeId`s
  (never positions, never entities) and survives both an unload and a refused
  load. `apply_airframe_damage` makes every node of the live scene carry the
  marker exactly when the state names it, so the pass is convergent and
  idempotent, a stale marker cannot survive a repair, and a freshly loaded
  generation inherits the damage without the damage being decided twice.
  `select_lod_presentation` (F11-B) then propagates the marker over the
  subtree, which is how one destroyed wing covers its pod, both of its LOD
  bands and the gun mounted on it (AC02). A part identity that names no node
  of the live scene is reported as `SceneEvent::UnknownDamage` and kept in the
  state, not silently dropped; with no live scene the pass is a no-op.
- **The viewer distance is not invented.** The load path does not create
  `LodDistance`: the camera/viewer stage (F21/F17) owns that input, and a
  scene with no viewer distance has no meaningful LOD selection. A schedule
  that runs `select_lod_presentation` without the resource fails loudly, which
  is the contract the resource's private field was built for (F11-B).

## Test inventory

| `accept_f11_c_*` test | Covers | Fails when |
| --- | --- | --- |
| `sockets_carry_roles_poses_and_provenance` (cs_content/tests/scene.rs) | the socket table over AC01's mirrored hierarchy: one socket per bound node in stable-id order, role/collision/zone/animation/provenance from the rule, the pose equals `world_transform`/`visual_transform`/`collision_transform` (mirror included), `sockets_of_role`, no socket for an unbound node, an explicit-unknown role listed by `unresolved_sockets` and in no role's list, and the unmatched rule still reported | the socket table is dropped, keyed by anything but the node id, given the local transform, defaults an unmeasured role, or loses the rule's provenance |
| `load_and_unload_the_same_airframe_hundred_times_without_leaking_entities` (cs_app) | **AC03**: 100 load/unload rounds; each load spawns exactly the 13 referenced nodes and reproduces round 1's *total* world entity count, each unload returns the world to its resting count and leaves no live record; generations count to 100 with no reuse; the recorded damage survives every teardown and the wing is `Disabled` (with its gun) until it is repaired in round 50; the log holds 200 events | teardown stops releasing entities, the load spawns something extra, a generation is reused, or damage does not follow the scene |
| `reload_releases_the_superseded_generation_and_leaves_no_old_root` (cs_app) | a reload: the exact `Loaded`/`Released`/`Loaded` log at generations 1 and 2, the old root entity is despawned, every live binding carries only the new generation, the root is a new entity, the damage recorded against generation 1 still holds on generation 2, and a second unload is a no-op that reports nothing | a reload leaves the old tree or the old record behind, reuses a root, or loses the damage |
| `a_refused_load_propagates_its_error_and_leaves_the_running_scene_alone` (cs_app) | error propagation and retry: a refusal before anything is live spawns nothing; three refusals (`ForeignContainer`, `UnknownNode`, unrepresentable `Transform`) each report the verbatim error at their own consumed generation (3, 4, 5) and change neither the live record nor the entity set; the retry then loads the same airframe at generation 6 | the error is swallowed, the running scene is torn down by a failed load, the generation is reused, or the retry is impossible |
| `sockets_are_bound_by_identity_and_survive_an_lod_transition` (cs_app) | the bound set is exactly the five evidenced sockets of the loaded subtree; the gun's role, collision role, zone, provenance, animation channel and mesh association are reachable by stable id; the socket pose equals the node's collision transform and the ECS affine; the pod with an unmeasured role gets no binding and stays unknown; the other root's gun is not imported or bound; across a 50 m → 150 m transition every socket's id, role, pose and generation are unchanged while the tail's band really swaps | sockets are bound by position, an unmeasured role is defaulted, another airframe's socket leaks in, or presentation writes identity/pose |
| `damage_marks_and_repairs_the_bound_part_subtree` (cs_app) | the marker is the damage identity on the named part only (the gun is disabled through its ancestor, not by a marker), the subtree is `Disabled` through presentation, the pass is convergent (a second run changes nothing and reports nothing), an unknown part id is reported and kept, a repair clears the marker and restores presentation while the distance verdict (the far band stays `LodCulled`) returns, and with no live scene the pass is a no-op that keeps the state | the damage state is ignored, markers are never cleared, unknown ids are dropped, or damage is applied to nothing |

**Sensitivity check (mutations applied and reverted while implementing, all
in the same session).**

| Mutation | Result |
| --- | --- |
| `release_scene` despawns nothing | AC03 fails: `the unload released every node of the scene: left: 13, right: 0`; the reload, refusal and damage tests fail too (4 failed, 1 passed) |
| the load no longer releases the superseded generation | the reload test fails and the refusal test fails (the stale tree makes the entity count wrong); AC03 still passes, as it should — it never reloads |
| the refusal arm logs nothing and swallows the error | the refusal test fails (4 passed, 1 failed) |
| an unmeasured socket role is bound like a known one | the socket test fails, and the reload test's expected `unresolved` list fails (3 passed, 2 failed) |
| the damage pass never writes a marker | the damage, AC03 and reload tests fail (2 passed, 3 failed) |
| the socket pose is the node's *local* transform | the content socket test fails on the composed linear/translation (`left: [[1,0,0],…,[0,0,-1]] translation [0.5,0,0]`, `right: the mirrored composed world map, translation [0,0,-2.5]`) and the app socket test fails (4 passed, 1 failed) |
| `unresolved_sockets` reports nothing | the content socket test fails on the `unresolved` assertion |

The tests are not vacuous.

## Unknowns and limitations (all recorded, none guessed)

- **The GameZ node-array reader still does not exist** (inherited from
  F11-B). **Affected content:** every original airframe and world hierarchy —
  nothing retail can be loaded through this path, so F11-D's roster audit
  (AC04) and F18's world import cannot claim original-data scene loading.
  **Resolving task:** #392 "Read the GameZ node array into ParsedNode records"
  (needs the owner to grant `crates/cs_formats/` owner paths). This limitation
  gates every scene-hierarchy fidelity claim and stays in force when this task
  is marked done.
- **What a damage identity *is* in the original is still unmeasured.** This
  stage reflects a recorded part identity onto the visual marker; it does not
  decide which hit damages which part. Whether the original's damage identity
  lives on the variant, on the parent or on the stored `zone_id` is unknown
  (F11-B's findings), and `zone_id`'s domain is still unknown (F11-A). The
  `zone_id` is carried through as raw data only. **Affected content:** which
  authored node covers which part of every airframe. **Resolving task:** F29
  ("zones, armor and system disablement") and F11-D's roster audit.
- **The socket vocabulary and the binding mechanism are designed engine
  contracts, not original data.** `PartRole`, `CollisionRole` and the
  authored-name-path rules are this engine's design; which original nodes hold
  which role is discovered per container by the evidence stages. Nothing here
  claims a real `corsair` wing is a damage zone.
- **A model name is still not proof of roster availability.** The loader
  imports the root an airframe *definition* references; it asserts nothing
  about player-selectability in any mode (F11 non-negotiable behavior 3).
  **Affected content:** the player-selectable roster and forced mission
  assignments. **Resolving task:** F11-D (AC04).
- **Damage visuals are a disable, not a damage look.** A destroyed part stops
  being presented (and the renderer will draw nothing for it, once F17 maps
  `NodePresentation`); whether the original shows fire, smoke, a detached wing
  or a scorch is unmeasured. **Resolving task:** F17 (drawable mapping) and
  F29 (system disablement).
- **The log grows with the number of requests.** `AirframeSceneLog` is
  append-only and is meant to be drained by a diagnostic surface; no draining
  policy is implemented, because no consumer owns it yet.
- **The request is served, but no production code inserts it yet.** The
  hand-off resource and its consumer exist and are exercised by the tests;
  the *producer* side — the mission/session bootstrap (`crates/cs_app/src/
  run.rs`) or the F15 `ReadyBundle` handoff (`crates/cs_app/src/loading.rs`)
  — is outside this stage's owner paths, so nothing in the running binary
  issues a load yet. **Affected content:** every airframe in the game (no
  scene is loaded outside tests). **Resolving task:** **#398** (F11-E) "Insert
  the airframe scene request from the real producer"; until it lands this
  stage is a tested library path, not a feature the game can show.
- **A refused load is not a queue.** One request is served per run; a producer
  that needs several loads must insert them over several runs (or a later
  stage will add a queue). The consequence is deliberate and loud: a second
  request inserted before the next run overwrites the first, which is why the
  log reports exactly what was served.
- **`NodeDisabled` ownership is now exclusive to the damage state.** Any other
  stage that wants to hide a node must record that in `AirframeDamageState`
  (or add a second, separately named record) — a marker inserted behind the
  damage pass's back is removed by the next pass. Recorded so F29/F20/F17 do
  not rediscover it.
- **Fixture scope** — synthetic only; this stage makes no retail compatibility
  claim.

## Sources used

- `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md` (F11-C section,
  deliverable, non-negotiable behaviors 1–5, AC01–AC04).
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, session generations,
  provenance, explicit unknowns, one pose owner, cycles invalid in ownership
  hierarchies).
- `docs/findings/2026-09-29-f11-b-hierarchy-import-and-lod-selection.md` (the
  import path, the all-or-nothing conversion, the `LodDistance` ownership it
  left to this stage, and the "damage identity across variants" and "`SceneImport`
  has no teardown" unknowns this stage closes).
- `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md` (the
  `BindingMap`/`SemanticBinding` input, the `scene_node` id scheme, the
  recorded node-flag and `zone_id` unknowns).
- Bevy 0.19 (`bevy_ecs-0.19.1/src/world/mod.rs`: `try_despawn` despawns
  `RelationshipTarget`s configured to despawn descendants; `hierarchy.rs`:
  "When a parent is despawned, all children (and their descendants) will also
  be despawned"; `iter_entities`; resource entities — one per resource plus the
  placeholder, which is why the AC03 test compares counts against a baseline
  instead of zero; `schedule/config.rs`: exclusive function systems enter a
  schedule through `IntoSystem` with an `IsExclusiveFunctionSystem` marker and
  chain like any other system).
