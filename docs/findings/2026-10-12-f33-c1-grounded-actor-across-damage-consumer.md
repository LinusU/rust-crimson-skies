# F33-C1: keeping a grounded actor grounded across the F29-C damage consumer

Date: 2026-10-12. Task: F33-C1 (#572) "Keep an F33 grounded actor grounded
across the F29-C damage consumer pass". Specs:
`specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`
(`### F33-C`) and `specs/F29-damage-zones-armor-destruction-and-bailout.md`
(`### F29-C`). Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no render,
no audio, so no `private/evidence/` report is produced and no original-data
claim is made.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/damage.rs` (extend): `apply_damage_state` reads the
  resolver's own lifecycle ledger before deciding a mount's fate;
  `apply_mount` takes the actor-level fact and closes rather than re-enables an
  intact mount on an actor that may no longer act; the private
  `lifecycle_ends_action` spells the ends-action split;
  `DamageConsumerEvent::MountGrounded`, `DamageConsumerReport::mounts_grounded`
  and the `is_noop` predicate carry the new record. Module docs updated.
- `crates/cs_app/tests/accept_f33_c_ally_lifecycle.rs` (**extend**): 4 new
  `accept_f33_c1_*` tests, added to this binary rather than to a target of its
  own for the measured disk cost the file's header records (a Bevy/Avian-linked
  test binary is ~123 MB, which exhausted the CI runner's disk twice on this
  branch).
- This file.

No protected path, no `cs_sim` change, no `cs_content` change, no original
data, no binary. The shared ordering/id-ownership contract was **not** edited:
the fix needed no new contract, so no owner-approved protected change was
requested.

**One observable failure, before the change.** Both consumers read the same
`cs_sim::damage::DamageResolver`, but they read different halves of it.
`cs_app::roster::apply_ally_lifecycle` (F33-C) grounds *every* mount of an
actor whose roster status ends action, so a lethal hit on a non-weapon node
takes the guns down. `cs_app::damage::apply_damage_state` (F29-C) is driven
only by `PartState`, and for an intact `SystemKind::Weapon` carrier it calls
`state.enable(mount)` — the convergence its own test
`accept_f29_c_the_consumer_pass_is_convergent_and_re_enables_a_stale_disable`
pins. In the synthetic fixture (`cs_content::damage::declared_synthetic_airframe_damage`)
`hull` is lethal and `gun_mount_1` is not, so a hull kill leaves the carrier
`Intact` while the actor is gone: running the damage pass after the lifecycle
pass re-enabled the guns the lifecycle pass had just disabled. Neither pass was
scheduled in an ECS system yet, so nothing observed it at runtime.

## The designed behavior

`apply_damage_state` now reads **both** halves of the resolver's authority for
one actor — the part states and the lifecycle ledger — and the weapon-gate half
of the pass is:

| part state | lifecycle ledger | gate |
| --- | --- | --- |
| `Destroyed` | any | mount disabled (`MountDisabled`) |
| `Intact` / `Damaged` | a transition that ends action | mount closed (`MountGrounded`) |
| `Intact` / `Damaged` | nothing that ends action | mount re-enabled (`MountEnabled`) |
| `Unknown` (unresolved pool) | any | asserted nowhere; refused by name |

"Ends action" is the designed split `cs_sim::allies::AllyStatus::ends_action`
and `cs_sim::targeting`'s `ends_targeting` already name — destruction, despawn
and mission removal. A capture and a bailout deliberately do not: a bailed-out
airframe is still a physical object and a capture changes *who owns* the actor,
not whether it exists, so an intact mount stays live on both (F33-C). The split
is spelled a third time here, as a private `lifecycle_ends_action` in
`cs_app::damage`, because `cs_sim` is outside this task's owner paths; folding
all three into one owner is a follow-up, not a scope creep.

The F29-C repair convergence is unchanged in scope: it now applies to actors
whose ledger does not end action, which is every living, captured and
bailed-out actor. `MountGrounded` is logged only when the gate actually
changes, so the pass stays convergent — a second pass over a grounded actor
reports nothing.

**The rejected alternative.** Option (b) of the task — "encode an explicit
pass-ordering contract and enforce it at the call site" — was rejected because
no call site exists: neither pass is scheduled in an ECS system yet, so a
documented order would be enforced nowhere, and the first wiring to get the
order wrong would silently reopen a dead actor's guns. Making the damage pass
lifecycle-aware makes the outcome order-independent by construction.

**Designed behavior, not original data.** Whether the original re-armed a
destroyed aircraft's intact gun mount, and which of destruction/despawn/mission
removal it treated as "can no longer act", are unmeasured (F29/F33 "Research
boundary"). **Affected content:** every actor whose damage graph declares a
weapon carrier and whose lifecycle ledger holds a transition that ends action.
**Resolving tasks:** F33-D and F29-D keep the original-family gates; nothing
here claims an original behaviour.

## Tests

Every test drives production code — `lower_graph` / `lower_policy`, the real
`DamageResolver` resolving a real `HitEvent`, `cs_app::weapons::lower_gun`,
`FireResolver`, `AirframeDamageState`, `lower_roster` / `open_roster` and
`apply_ally_lifecycle` — never a test-only bridge. The firing half is observed
off the gate itself: a `FireIntent` is refused with `MountDisabled` and
consumes no round.

| test | what it pins |
| --- | --- |
| `accept_f33_c1_the_damage_pass_does_not_re_enable_a_grounded_actors_mount` (minimum) | the harmful order — lifecycle pass first, damage pass second — after a hull-lethal hit: the actor's intact mount stays disabled, the damage pass reports no `MountEnabled`, a later shot is refused with `MountDisabled` consuming no round, and a second damage pass is a no-op |
| `accept_f33_c1_the_damage_pass_alone_grounds_a_destroyed_actors_intact_mount` | the other order, and the single-pass case: an intact carrier on an actor that may no longer act is closed and named `MountGrounded` (with the hull's own `VisualDestroyed`), the shot is refused, a second pass is a no-op, and the lifecycle pass then finds the gate already closed and disables nothing more |
| `accept_f33_c1_a_living_actors_stale_mount_disable_is_still_cleared` | F29 repair semantics preserved: a scratched, living actor's stale disable is cleared as `MountEnabled` and the gun really consumes a round |
| `accept_f33_c1_a_captured_actors_intact_mount_is_not_grounded` | the guard is exactly the ends-action split: a capture is recorded, the actor still acts, and its intact mount is neither closed by the damage pass nor left disabled |

`accept_f29_c_the_consumer_pass_is_convergent_and_re_enables_a_stale_disable`
in `crates/cs_app/tests/accept_f29_c_damage_consumers.rs` is untouched and still
passes; it is the positive control for the repair direction, and its actor
records no lifecycle transition, so the guard does not apply to it.

## Mutation probes

Each probe edited one production line, ran the F33-C selection, recorded the
failing tests and restored the file from a saved copy:

| probe | edit | result |
| --- | --- | --- |
| the guard is removed | `apply_mount`'s `else if grounded` disabled via `else if false && grounded` | `accept_f33_c1_the_damage_pass_does_not_re_enable_a_grounded_actors_mount` failed (`mounts_enabled` 1, the dead wingmate's guns reopened) and `accept_f33_c1_the_damage_pass_alone_grounds_a_destroyed_actors_intact_mount` failed (`mounts_grounded` 0) |
| — | the same probe, negative controls | `accept_f33_c1_a_living_actors_stale_mount_disable_is_still_cleared` and `accept_f33_c1_a_captured_actors_intact_mount_is_not_grounded` passed, so the probe is not satisfied by a blanket disable |

## Considered and excluded

`apply_propulsion_state` (F29-C.1) has the same shape — an `Enabled` propulsion
system restores a cut thrust authority — and was considered for the same guard
and deliberately left alone. Its restore branch requires
`thrust_authority == 0.0`, which only a previously `Disabled` propulsion system
can have written, which only a destroyed engine can cause, and the resolver
offers no repair path (`DamageResolver` has no reverse transition: a
`PartState` moves toward `Destroyed` and never back). The branch is therefore
unreachable for a grounded actor, and adding a guard would be a change with no
observable behavior.

## Follow-ups left open

- **One owner for the ends-action split.** The rule is now spelled three times:
  `cs_sim::allies::AllyStatus::ends_action`, `cs_sim::targeting`'s private
  `ends_targeting` and `cs_app::damage`'s private `lifecycle_ends_action`.
  Consolidating it in `cs_sim` (where `LifecycleKind` lives) needs its own task
  with the owner paths to do it. The three copies agree today; a fourth reader
  that guesses a different set is the risk.
- **ECS wiring.** Neither pass has a system yet. When they are wired, the
  order-independence this change buys has to be preserved (and asserted), not
  traded back for a "run lifecycle first" comment.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`
  (`### F33-C`), `specs/F29-damage-zones-armor-destruction-and-bailout.md`
  (`### F29-C`), `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-10-02-f29-c-damage-consumers.md` (the pass, its
  convergence contract and its refusal-log pattern).
- `docs/findings/2026-10-03-f33-c-ai-roles-dialogue-voices-and-mission-callbacks.md`
  (the F33-C seam and the unresolved F33-D retail question about whether a
  captured or bailed-out aircraft may still fire — untouched here).
- `crates/cs_sim/src/damage/{resolver,events}.rs`,
  `crates/cs_sim/src/allies.rs`, `crates/cs_sim/src/targeting.rs`,
  `crates/cs_app/src/damage.rs`, `crates/cs_app/src/roster.rs`,
  `crates/cs_content/src/damage.rs`.
