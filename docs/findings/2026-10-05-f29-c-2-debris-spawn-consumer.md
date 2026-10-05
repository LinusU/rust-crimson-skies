# F29-C.2: spawning authored debris from authoritative destruction

Date: 2026-10-05. Task: F29-C.2 "Spawn authored debris when a damage part is
destroyed" (#518). Spec:
`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage `### F29-C`.
Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no render, no audio, so no
`private/evidence/` report is produced.

## Why this stage exists

F29-C (#107) wired the resolver's authoritative part state to two consumers —
the weapon firing gate and the visual damage record — and stopped there. Its
finding names debris explicitly as an open follow-up: "A destruction should
spawn authored debris/wreck visuals; the debris pass and its asset bindings
are another stage's"
(`docs/findings/2026-10-02-f29-c-damage-consumers.md`). So after F29-C a
destroyed part changed its visuals and its gate, and spawned nothing: `cs_app`
had no debris record at all, no place to hang an authored wreckage object, and
no refusal channel that could name an authored binding the content boundary
had not resolved.

This task is that follow-up. It is one bounded slice: the decision and the
record of a debris instance. It does **not** load a wreckage asset, choose a
mesh, place a pose, give the wreckage a body, or claim the original did any
of this.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/debris.rs` (**new**): the debris consumer —
  `PartDebrisBinding`, `SpawnedDebris`, `DamageDebrisRefusal`,
  `DamageDebrisEvent`, `DamageDebrisLog`, `DamageDebrisReport`,
  `DamageDebrisOutcome`, `apply_debris_state`, `release_debris`, and the
  private binding/instance indexes and spawn/despawn helpers. `cs_app` is the
  debris/spawn pass crate: its `scene` module owns the scene import these
  bindings ride beside, and `damage` owns the sibling consumer seam.
- `crates/cs_app/src/lib.rs` (wiring only): `pub mod debris;` and the one
  paragraph in the crate docs naming the new consumer.
- `crates/cs_app/src/damage.rs` (doc only): the F29-C consumer comment now
  names `crate::debris` as the debris consumer instead of listing debris among
  the consumers that "do not exist on this branch yet". No code in `damage.rs`
  changed.
- `crates/cs_app/tests/accept_f29_c_debris_spawn.rs` (**new**): the 9
  `accept_f29_c_debris_*` tests.
- This file.

No protected path, no `cs_content` / `cs_sim` change, no original data, no
binary.

**One observable failure, before the change:** after the real resolver
destroyed `engine_1`, the world held zero debris entities — `cs_app` had no
debris record type and no pass that could spawn one, so the count of wreckage
records after a destruction was `0` where the designed rule requires `1`, and
an unresolved authored binding had nowhere to be reported. Probe A below
reproduces exactly that state against the finished code (`left: 0, right: 1`
on the instance count).

## The designed rule

`apply_debris_state(world, resolver, actor) -> DamageDebrisOutcome` reads the
resolver's own `part_state` — never an event replay, never a particle (F29
non-negotiable 1) — and converges each part of the actor's registered graph on
one debris instance:

| authority state for the part | `PartDebrisBinding` | what the pass does |
| --- | --- | --- |
| `Destroyed` | `Resolved::Known(object)` | exactly one [`SpawnedDebris`] for `(actor, node)`, carrying `object` and the binding's `SceneGeneration`; a duplicate or a superseded generation is despawned first |
| `Destroyed` | `Resolved::Unknown` | refused by name with its claim (`UnresolvedDebris`); nothing spawned, nothing despawned |
| `Destroyed` | none | skipped silently: nothing authored debris for this part, and absence is not an error |
| `Intact` / `Damaged` | any | every instance the part owns is despawned (the repair/restart half) |
| `Unknown` / node the resolver does not know | any | refused by name with its claim (`UnresolvedIntegrity`); neither direction asserted |
| any, actor from another session or unregistered | any | refused before a single part is read (`ForeignSession` / `UnknownActor`) |

The two teardown halves are separate on purpose:

* **repair / restart** is state-driven inside `apply_debris_state`: the
  authority no longer calling the part destroyed despawns its instance, and
  that happens even when the binding entity is gone with it, because the
  authority — not the binding — decides what exists. Running the pass again is
  a no-op.
* **reload / swap** is the explicit entry `release_debris(world, actor)`: it
  despawns every instance the actor owns, reports how many went, and reports
  `0` on a second call. The scene release path rebuilds entities and cannot
  name the actor a leftover belongs to, so the caller that owns the session's
  actors releases debris beside their other bindings (the same shape as
  `repair_damage_zone` being the repair path's entry).

`SpawnedDebris` carries **no** mesh, pose, velocity or collider. Which
authored object `source` resolves to, where the wreckage sits and how it moves
are the debris presentation stage's decisions, exactly as the task states
("Debris assets and the spawn path are another stage's"). The instance is the
record this consumer owns; it is stamped with the scene generation so a
leftover is identified by mismatch rather than by a surviving pointer (the
`STATE-TRANSACTIONS` generation discipline).

**Designed behavior, not original data.** Whether the original spawned
authored debris when a damage part was destroyed, which object it used, how
many instances it produced and whether a repair removed them are
**unmeasured** — F29's "Research boundary", and F29-D (#108) keeps the
original-family gate (it is blocked on opcode vocabulary and on an owner
ruling). The rule above — one instance per destroyed part that carries an
authored, resolved binding, gone again when the authority stops calling the
part destroyed — is this engine's designed behavior. **Affected content:**
every airframe and world object whose damage graph a session registers, as
soon as its spawn path publishes `PartDebrisBinding`s; today that is the
synthetic fixture parts (`engine_1`, `gun_mount_1`) the tests bind, and no
retail airframe has bindings yet, so no retail content changes. **Resolving
tasks:** F29-D validates against original data and requires `retail`.

## Tests

Every test drives production code — `cs_app::damage::lower_graph` /
`lower_policy`, the real `DamageResolver`, `apply_debris_state` and
`release_debris` — never a test-only bridge, and every observation is read off
the `SpawnedDebris` entity the production pass spawned. Task test prefix:
`accept_f29_c_debris_` (the stage prefix `accept_f29_c_` selects them too).

| test | what it pins |
| --- | --- |
| `accept_f29_c_debris_destroying_a_part_spawns_its_authored_debris_once` (minimum) | an intact part spawns nothing; destroying `engine_1` spawns exactly one instance carrying the authored object, the node, the actor and the binding's generation |
| `accept_f29_c_debris_a_second_pass_spawns_none` | the second pass is a no-op and keeps the same entity; a duplicate instance left in the world is collapsed back to one without spawning anything new |
| `accept_f29_c_debris_a_repair_removes_the_spawned_debris` | an authority that no longer calls the part destroyed despawns the instance — even with the binding entity already gone — and a second run is a no-op |
| `accept_f29_c_debris_a_reload_releases_every_spawned_debris` | `release_debris` reports 2 for two destroyed parts, leaves another actor's debris alone, and reports 0 the second time |
| `accept_f29_c_debris_a_part_without_a_binding_spawns_nothing` | absence of an authored binding is skipped, never refused and never guessed |
| `accept_f29_c_debris_an_unresolved_binding_is_refused_by_name` | `Resolved::Unknown` is refused with its claim and reason, `spawned == 0`, no entity appears |
| `accept_f29_c_debris_an_unresolved_pool_asserts_neither_direction` | an unmeasured integrity refuses by name and leaves an existing instance exactly as it was |
| `accept_f29_c_debris_a_foreign_or_unknown_actor_is_refused_by_name` | a foreign session generation and an unregistered actor are refused before any part is read |
| `accept_f29_c_debris_a_superseded_binding_replaces_the_instance` | a binding respawned under a live generation replaces the stale instance: `spawned == 1, despawned == 1`, and no instance of the stale generation survives |

## Mutation probes

Each probe edited one production line, ran the task selection, recorded the
failing tests and restored the file from Git:

| probe | edit | result |
| --- | --- | --- |
| the pass stops spawning | `if !kept` → `if false && !kept` in `apply_debris_state` | 5 failed: `..._destroying_a_part_spawns_its_authored_debris_once`, `..._a_second_pass_spawns_none`, `..._a_repair_removes_the_spawned_debris`, `..._a_reload_releases_every_spawned_debris`, `..._a_superseded_binding_replaces_the_instance` (instance count 0) |
| the pass stops despawning | `world.entity_mut(instance.entity).despawn();` removed from `despawn_instance` | 3 failed: `..._a_repair_removes_the_spawned_debris`, `..._a_second_pass_spawns_none`, `..._a_superseded_binding_replaces_the_instance` (2 instances left) |
| the unresolved-binding refusal is dropped | the `unresolved_debris` call replaced by a bare `continue` | 1 failed: `..._an_unresolved_binding_is_refused_by_name` (`log: []`) |
| the reload teardown stops despawning | `world.entity_mut(entity).despawn();` removed from `release_debris` | 1 failed: `..._a_reload_releases_every_spawned_debris` |

## Unknowns

- Whether the original spawned authored debris per destroyed part, which
  object, how many instances, and whether a repair or a restart removed them:
  **unmeasured**. `retail` file access measures damage *regions* and wreck
  *materials* (see #521 / F29-D.1) but nothing that says a runtime spawn rule;
  F29-D (#108) is blocked pending opcode vocabulary and an owner ruling.
  Nothing in this stage claims an original behaviour.
- The debris asset and its spawn path (mesh, material, pose, physics) are
  another stage's; `SpawnedDebris::source` names the object but this stage
  never resolves it.

## Follow-ups left open (filed with `create_tasks`)

- **Scheduling the debris pass.** `apply_debris_state` / `release_debris` are
  production seams in the shape F29-C's own `apply_damage_state` and
  `apply_damage_events` already have: real implementations, driven directly by
  their tests, with no schedule system calling them yet. Whoever owns the
  runtime damage tick must call them beside the other consumers, and the
  session/scene teardown must call `release_debris` beside the other release
  work.
- **Publishing the bindings.** `PartDebrisBinding` is inserted by the spawn
  path, exactly as `DamageZoneBinding` is; neither is inserted by a production
  spawn path yet. The stage that authors the wreckage objects owns those
  insertions.
- **Presenting an instance.** Resolving `SpawnedDebris::source` to a rendered,
  simulated wreck (pose, physics, lifetime) is the debris presentation
  stage's.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-C`,
  non-negotiable 1, "Research boundary"), `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-10-02-f29-c-damage-consumers.md` (the consumer pattern
  this pass follows, and the debris follow-up it leaves open).
- `docs/findings/2026-10-02-f29-damage-zone-collider-call.md` (the binding +
  refusal-log + teardown-entry shape).
- `crates/cs_app/src/ordnance.rs::sync_ordnance_mirrors` (the
  reconcile-a-record precedent: identity + generation stamp, convergent
  spawn/despawn of a mirror entity).
- `crates/cs_sim/src/damage/{resolver,graph,events,synthetic}.rs`,
  `crates/cs_app/src/damage.rs`, `crates/cs_app/src/scene.rs`.
