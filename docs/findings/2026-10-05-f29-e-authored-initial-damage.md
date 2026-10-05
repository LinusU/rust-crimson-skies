# F29-E: authored initial damage into part state (#515)

## What was added

* `cs_sim::damage::DamageResolver::apply_initial_damage` (types in
  `cs_sim::damage::initial`): starts a registered actor's *known* pools with
  authored integrity removed, returning an `InitialDamageReport` of applied
  and unresolved statements.
* `cs_app::damage::apply_world_initial_damage`: the boundary from a
  `WorldInstance`'s `initially_damaged` set plus a caller-supplied
  `WorldDamageMapping` to those statements; damaged objects with no mapping
  are listed in `unmapped_objects`.
* Tests `accept_f29_e_*` in `crates/cs_app/tests/accept_f29_e_initial_damage.rs`.

## Refused, never guessed

Unknown node, unresolved (`Resolved::Unknown`) pool, non-finite or
non-positive amount, repeated node in one batch, and an amount equal to or
above the pool (`PreDestroyed`). Nothing is clamped. Application to an actor
that is closed, has a lifecycle record or has any damaged pool is an error
(`InitialDamageTooLate`).

## Unknowns left open

1. **World object -> damage node mapping.** `WorldInstance::initially_damaged`
   is a bare set of object ids. No original evidence of which object is which
   damage node exists in this repository; the mapping is an input, and no
   production content supplies one yet. Affects every mission with authored
   initial damage. Resolving task: whichever mission import supplies the
   mapping with provenance (to be filed by the first mission needing it).
2. **Authored amount.** The load record has no amount. Whether original
   "initially damaged" means a fraction, a fixed state or a visual variant is
   unknown; the amount is likewise an input with its own provenance.
3. **Pre-destroyed start.** Whether the original game can start an object
   destroyed is unknown, so such a statement is refused rather than lowered
   to a destroyed part (no `Destroyed` lifecycle event is emitted).
4. **Visual/collider consumers** are not re-run here; callers still run
   `apply_damage_state` after registration to present the damaged parts.

Finding F29-A follow-up 5 is resolved by this task for the mechanism only;
items 1-3 gate any fidelity claim about initial damage.
