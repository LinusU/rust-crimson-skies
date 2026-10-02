# F29: a destroyed damage zone decides and calls the collision seam

Date: 2026-10-02. Task: "F29 damage zones decide and call
`remove_collider_for_damage` / `restore_collider_after_repair`" (#511). Spec:
`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stages `### F29-B` /
`### F29-C`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no render,
no audio, so no `private/evidence/` report is produced.

## Why this stage exists

F20-C.04 (#504) added the collision-side seam for a destroyed part
(`cs_app::physics::remove_collider_for_damage` /
`restore_collider_after_repair`) and recorded that **nothing called it**:
`docs/findings/2026-10-02-f20-c-04-collider-presence-consumer.md`. The seam
carries no gameplay rule on purpose — *whether* a destroyed part loses its
collider is F29's decision — so until F29 calls it the answer to "does a
destroyed part still collide?" is whatever the spawner and the clip say.

This stage is the caller. It is a bounded slice of F29-B/C: it does not build
the armor/disablement model or the bailout path, and it does not invent which
original parts are surfaces. It adds the record that ties a collider-managed
entity to its damage zone, the designed decision that maps a zone's part
transition onto the seam, and the log that makes a refusal visible.

## Files

- `crates/cs_app/src/damage.rs` (extend): `DamageZoneBinding`,
  `ZoneColliderDecision`, `zone_collider_decision`, `DamageColliderEvent`,
  `DamageColliderLog`, `DamageColliderReport`, `apply_damage_events`,
  `repair_damage_zone`, and the private entity-lookup/application helpers. The
  module is F29's app-side owner path.
- `crates/cs_app/src/lib.rs` (wiring only): one sentence in the `damage`
  paragraph naming the new bridge.
- `crates/cs_app/tests/accept_f29_b_collider_call.rs` (**new**): the 7
  `accept_f29_b_collider_*` tests.
- This file.

`crates/cs_app/src/physics/collider.rs` and `crates/cs_app/src/animation/` were
read-only: the bridge imports the seam from `crate::physics` and never writes
the animation or collision records itself.

**One observable failure, before the change:** the real session resolver can
destroy a damage zone and emit its `PartTransition{to: Destroyed}`, and the
zone's real Avian collider stays in the simulation — a body flying at the
destroyed part is stopped and the production contact reporter reports the
contact. Nothing consumes the resolution into the collision seam.

## The designed rule

`zone_collider_decision` is the whole rule and it is pure:

| from | to | decision |
| --- | --- | --- |
| `Intact` or `Damaged` | `Destroyed` | `Remove` |
| `Destroyed` | `Intact` or `Damaged` | `Restore` |
| anything else | anything else | none |

A zone that is only scratched (`Intact` ↔ `Damaged`) keeps its collider; an
unresolved (`Unknown`) state is never guessed into a decision. The bridge looks
the entity up from the zone's **own** record, `DamageZoneBinding { actor, node
}`, which is put on the entity the collision policy manages — the same entity
that carries `NodeColliderPresence` and Avian's `Collider`.

**This is designed behavior, not original data.** Whether the original removes
a destroyed part's collider, and which original parts are surfaces at all, is
unmeasured (the manual and guides name no damage equations; F29's research
boundary). **Affected content:** every destroyed part's collision surface for
every airframe and world object. **Resolving task:** F29-D (and F20-D for the
visibility coupling) — the two families keep the validation gate, and nothing
here claims an original behaviour.

## The decision is F29's record, not the animation's

`apply_damage_events` reads only `DamageEventKind::PartTransition` from the
resolver's output. It never reads `NodeDisabled`, `NodePresentation` or
`NodeAnimatedVisibility`, so the collider decision has one owner and cannot race
the clip-vs-damage priority. The resolver's node keys are graph-local, so the
binding carries the session-qualified `ActorId` too.

`apply_damage_events(world, actor, events)` takes the victim `actor` explicitly:
a `PartTransition` names only the node, not its actor, while one
`resolve` batch can cover several actors. The F29-B/C producer calls it once per
resolved victim. The bridge is a plain function, not a system, for the same
reason `bind_animated_node` is (`docs/findings/2026-10-02-f20-c-wired-session-integration.md`):
the spawn/resolution path owns the identity and performs one atomic step.

## Error handling: refusals are logged, not swallowed

`ColliderDecisionError` is handled arm by arm, as the task requires:

- `UnknownEntity` — the bound node is gone. A no-op; recorded as
  `DamageColliderEvent::ReleasedNode` so the release is visible rather than
  silent. The seam changed nothing and neither does the bridge. This arm is
  **defensive**: the bridge resolves a zone from a *live* `DamageZoneBinding`,
  so a zone whose entity the teardown released is indistinguishable from one the
  spawn path never bound and currently surfaces as `UnboundZone`, not
  `ReleasedNode`. The arm stays because the seam's error type can carry it and
  the match must stay exhaustive; a caller that holds an entity handle is what
  would produce it.
- `UnmanagedNode` — the entity carries no `NodeColliderPresence`, so the spawner
  never put its collider under the policy. This is a **wiring gap**, reported as
  `DamageColliderEvent::UnmanagedNode` through `DamageColliderLog`, the way
  F11-C reports `SceneEvent::UnknownDamage` through `AirframeSceneLog`. A
  decision that silently disappears is worse than one that is refused, and this
  is the signal that the spawn-wiring follow-up (#510) is incomplete.
- A zone no entity is bound to is `DamageColliderEvent::UnboundZone`, also
  reported: the damage record knows a part the spawn path never bound.

The log grows with decisions that changed state or were refused, never with
frames.

## Keeping the animation side's authority intact

The seam is terminal for `apply_collider_presence`, so the bridge never has to
re-assert a removal: once `remove_collider_for_damage` has put the record at
`RemovedByDamage`, a looping clip that re-shows the node leaves it alone, and a
later `apply_damage_events` call for the same transition is a no-op. The
tests assert both directions, not only the removal:

- `..._a_looping_clip_cannot_restore_a_destroyed_zones_collider` drives the real
  breakable clip through two full loops (re-shows and re-hides counted) and the
  collider stays out of the simulation; a probe flies through.
- `..._a_repair_under_a_still_hiding_clip_does_not_expose_the_node` repairs while
  the clip still hides the node and the record lands on `HiddenByAnimation`, not
  `Live`; a probe flies through until the clip's show tick.
- `..._applying_the_same_transition_twice_changes_nothing` pins idempotence on
  the report and the log.

## Tests

| test | what it pins |
| --- | --- |
| `accept_f29_b_collider_a_destroyed_zone_removes_the_collider_and_a_repair_restores_it` (minimum) | the real `DamageResolver` destroys the zone; `apply_damage_events` reports exactly one removal and the plan probe flies through; `repair_damage_zone` restores it and a probe is stopped again |
| `accept_f29_b_collider_a_looping_clip_cannot_restore_a_destroyed_zones_collider` | the animation's authority: the real clip loops, re-shows and re-hides counted, the damage removal stays terminal |
| `accept_f29_b_collider_a_repair_under_a_still_hiding_clip_does_not_expose_the_node` | the repair recomputes through the clip's current verdict; the clip's show tick restores it |
| `accept_f29_b_collider_applying_the_same_transition_twice_changes_nothing` | idempotence on the report counts and the log length |
| `accept_f29_b_collider_an_unmanaged_zone_is_reported_not_swallowed` | `UnboundZone` and `UnmanagedNode` are both logged; the unmanaged collider and marker are untouched |
| `accept_f29_b_collider_a_damaged_but_intact_zone_keeps_its_collider` | a scratch does not move a collider; the report is a no-op and the log is empty |
| `accept_f29_b_collider_the_rule_ignores_non_destroying_transitions` | the pure decision table |

Every test drives the production resolver through the real fixed loop and reads
the production contact reporter or Avian's own marker; none calls
`remove_collider_for_damage` directly.

## Mutation probes

Each probe edited one production line in `crates/cs_app/src/damage.rs`, ran
`cargo test -p cs_app --locked --test accept_f29_b_collider_call`, recorded the
failing tests and reverted the edit with `git checkout --`:

| probe | edit | result |
| --- | --- | --- |
| the bridge decides nothing | `apply_damage_events` iterates no events, applying nothing | 5 failed: `..._a_destroyed_zone_removes_the_collider_and_a_repair_restores_it`, `..._a_looping_clip_cannot_restore_a_destroyed_zones_collider`, `..._a_repair_under_a_still_hiding_clip_does_not_expose_the_node`, `..._applying_the_same_transition_twice_changes_nothing`, `..._an_unmanaged_zone_is_reported_not_swallowed` |
| the rule only removes | the `Destroyed → Intact/Damaged` arm of `zone_collider_decision` returns `None` | `..._the_rule_ignores_non_destroying_transitions` failed |
| the refusal is swallowed | the `UnmanagedNode` arm returns without logging | `..._an_unmanaged_zone_is_reported_not_swallowed` failed |

Probe two also locates the boundary: `repair_damage_zone` is the repair path's
explicit entry and decides `Restore` itself, so a broken rule arm is caught by
the pure table test while the repair entry stays green; a bridge that applied
nothing (probe one) fails every contact-sensitive test.

## Unknowns

- **Whether the original couples destruction to collision at all, and which
  parts are surfaces, is unmeasured.** This stage's rule is designed; F29-D and
  F20-D keep the gate. **Affected content:** every destroyed part's collision
  surface.
- **Which entities are born with a `DamageZoneBinding` is not decided here.**
  The spawn path (F29-B) inserts it for a part whose collider the policy
  manages; until then the bridge reports `UnboundZone`, which is the intended
  loud state.
- **`NodeDisabled` is not a collider input.** F11-C states collision reads its
  own record; inferring the collider from the presentation marker here would put
  a second reader on the clip-vs-damage priority.

## Follow-ups

- **The producer that fills the resolution.** The F29-B/C session path must call
  `apply_damage_events` per resolved victim once the resolver is wired into the
  ECS schedule (F29-A follow-up 2). This stage provides and tests the consumer,
  not the producer.
- **The spawn wiring** (#510) must insert `DamageZoneBinding` (and
  `NodeColliderPresence`) for the parts whose colliders the policy manages, on
  the same entity the clip's `NodeAnimatedVisibility` and Avian's `Collider`
  live on.
- **Networked authority** over collider state (F57) is a separate concern.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-B`,
  `### F29-C`, AC02/AC03), `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-10-02-f20-c-04-collider-presence-consumer.md` (the seam
  this stage calls, its terminal damage state and its split probe lanes),
- `docs/findings/2026-09-30-f29-a-damage-graphs-hit-ordering-lifecycle.md` (the
  resolver, its ordered `PartTransition` output and its follow-ups),
- `crates/cs_sim/src/damage/{resolver,events,graph,synthetic}.rs` (the
  resolver output this stage consumes),
- `crates/cs_app/src/physics/collider.rs` (the seam; read-only here),
  `crates/cs_app/src/scene.rs` (`AirframeSceneLog`/`SceneEvent::UnknownDamage`,
  the reporting shape this stage mirrors; read-only here).
