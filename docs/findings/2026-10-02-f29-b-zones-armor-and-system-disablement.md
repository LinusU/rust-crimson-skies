# F29-B: zones, armor and system disablement

Date: 2026-10-02. Task: F29-B "Implement zones, armor and system disablement"
(`specs/F29-damage-zones-armor-destruction-and-bailout.md`, section
`### F29-B`). Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no
render, no audio, so no `private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/damage/resolver.rs` (extend): the armor-guard fallback in
  `apply_hit` and the new `DamageResolver::system_state` /
  `DamageResolver::disabled_systems` accessors plus the private
  `ActorDamage::system_state`.
- `crates/cs_sim/src/damage/graph.rs` (extend): the `SystemState` vocabulary
  and `SystemKind::ALL`.
- `crates/cs_sim/src/damage/mod.rs` (wiring + docs): re-export `SystemState`.
- `crates/cs_sim/tests/accept_f29_b_zones_armor_disablement.rs` (**new**): 9
  `accept_f29_b_*` resolver tests.
- `crates/cs_app/tests/accept_f29_b_lowered_damage_path.rs` (**new**): 3
  `accept_f29_b_*` tests over the declared → lowered → resolver path.
- This file.

No protected path, no `cs_content` change, no original data, no binary.

**One observable failure, before the change:** an `Armor`-channel shot enters
the guard named by the target's `guarded_by` edge, and the resolver's
`apply_hit` follows only the guard's own `overflow`. A guard that declares no
overflow therefore *swallows the shot's remainder*: a 15-damage armor shot at
a 40-point hull behind a 10-point plate left the hull at full integrity and
the five overkill points vanished. The same shape means one armor zone cannot
guard several parts, because every shot's remainder would have to flow to the
single authored `overflow` target rather than to the part the shot named.
Second gap: the only record of system disablement was the transient
`DamageEventKind::SystemDisabled` event — a gate had to replay a session's whole
event history to learn that a mount's weapon is still down, and nothing
answered `None` (no such system) separately from `Unknown` (unmeasured).

## What F29-A had already delivered

F29-A did more than "define the typed contract": its resolver already routes
`DamageChannel::Armor` through `guarded_by`, applies `min(remaining, pool)`
per hop, and emits `PartTransition` + `SystemDisabled` when a carrier's pool
reaches zero. The F29-B *minimum* scenario (armor and internal channels
produce distinguishable results without a multiplier) was therefore already
implemented and covered by
`accept_f29_a_armor_and_internal_channels_route_distinguishably`. This stage
does not re-implement that; it closes the two real gaps above and pins the
F29-B scenario on the full declared → lowered → resolver path, which no
F29-A test drove for armor or disablement.

## The designed behavior

### Guard fallback (armor overkill)

`apply_hit` records the node the shot named as the chain's **fallback** when
the shot entered through that node's guard. At each hop:

| condition | next hop |
| --- | --- |
| the guard declares `overflow` | the authored `overflow` target (unchanged) |
| the guard declares no `overflow` | the node the shot named |
| neither | the chain ends |

The authored `overflow` edge still wins when declared, so the F29-A synthetic
fixture (`nose_armor.overflow = hull`) behaves exactly as before, and a guard
protecting several parts now routes each shot's overkill into the part that
shot named. This is a routing rule, not a damage factor: no multiplier is
introduced anywhere.

### System state

`SystemState` is `Enabled`, `Disabled` or `Unknown`, derived from the same
part states the `SystemDisabled` events come from:

- a carrier that is `Destroyed` makes the system `Disabled` — exactly the
  transition whose event fired;
- when no carrier is destroyed but at least one carrier pool is
  `Resolved::Unknown`, the system is `Unknown` — asserted nowhere;
- otherwise `Enabled`.
- `None` when the actor is unknown or no node declares the system: "no such
  system" is not "the system is down".

`disabled_systems` is the set of systems currently `Disabled`, in
`SystemKind::ALL` order.

**This is designed behavior, not original data.** The aggregation is the
union of the declaring parts, which mirrors the per-node `SystemDisabled`
event; a game that treats several engines as one scaled propulsion authority
is a distinct authored rule and is not invented here. Whether the original
routed armor the way this model does, whether a destroyed part kept its
overkill, and how it aggregated carriers are **unmeasured** (the manual and
guides name no damage equations; F29's research boundary). **Affected
content:** armor routing and overkill for every airframe and world object, and
the meaning of a disabled system for every carrier. **Resolving tasks:** F29-D
(and F29-C for the consumer wiring) keep the original-family gate; nothing
here claims an original behaviour.

## Tests

Every test drives production code — the resolver, `lower_graph` /
`lower_policy` and the graph validators — never a test-only reimplementation.

| test | what it pins |
| --- | --- |
| `accept_f29_b_armor_and_internal_channels_produce_distinguishable_results` (minimum) | the same raw damage on the two channels leaves pools that differ by exactly the armor's absorbed share; the internal hit leaves the armor `Intact` |
| `accept_f29_b_armor_overkill_continues_into_the_guarded_part` | a guard with no `overflow` passes its five overkill points into the hull |
| `accept_f29_b_one_armor_zone_routes_each_shots_overkill_to_its_own_part` | one shared zone guards a wing and a tail; each shot's remainder lands on its own part, and a depleted zone routes the whole second shot |
| `accept_f29_b_internal_channel_bypasses_the_guard` | the named part takes the full internal damage and the guard stays intact |
| `accept_f29_b_armor_channel_on_an_unguarded_part_lands_directly` | the armor channel is a route, not a blanket shield |
| `accept_f29_b_destroying_a_carrier_disables_only_its_own_system` | mount → `Weapon` disabled, `Propulsion` enabled; then engine → both disabled; `disabled_systems` agrees |
| `accept_f29_b_system_state_tracks_damage_destruction_and_unknown` | a scratch keeps a system enabled; an unresolved carrier pool is `Unknown` |
| `accept_f29_b_system_state_is_none_when_no_part_declares_the_system` | an engine-only graph has no weapon system; an unregistered actor has none at all |
| `accept_f29_b_refusals_stay_named_on_the_armor_channel` | unknown actor/node are named refusals while the valid armor hit still resolves |
| `accept_f29_b_lowered_graph_routes_armor_and_internal_distinguishably` | the declared fixture lowers and the two channels then diverge end-to-end |
| `accept_f29_b_lowered_graph_reports_a_disabled_system` | the lowered mount's destruction emits `SystemDisabled` and flips the queryable state |
| `accept_f29_b_lowered_guard_without_overflow_keeps_the_overkill` | a declared guard with no overflow keeps the overkill; an unknown attribution still refuses at the boundary |

## Mutation probes

Each probe edited one production line, ran the F29-B selection, recorded the
failing tests and restored the file from a saved copy:

| probe | edit | result |
| --- | --- | --- |
| the guard swallows overkill | `apply_hit`'s fallback forced to `None` | `accept_f29_b_armor_overkill_continues_into_the_guarded_part` and `accept_f29_b_one_armor_zone_routes_each_shots_overkill_to_its_own_part` failed |
| system state ignores carriers | `ActorDamage::system_state`'s `Destroyed` arm made a no-op | `accept_f29_b_destroying_a_carrier_disables_only_its_own_system` failed |

## Follow-ups left open

- **Authored initial damage.** F29-A finding follow-up 5 ("applying a
  mission's authored initial damage needs a registration variant") is still
  open: `register_actor` starts every pool pristine, and the
  `WorldInstance::initially_damaged` records are not yet lowered into part
  state. Filed as follow-up #515 rather than guessed here, because the
  world-object → damage-node mapping and the legal pre-destroyed state are
  unmeasured.
- **Consumer wiring.** The `SystemDisabled`/`PartTransition` events still need
  their producers and consumers (`WeaponState` firing gate,
  `AirframeDamageState` visuals, debris, scoring, bailout). That is F29-C
  (#107) plus the #510 spawn wiring; this stage provides the queryable state
  F29-C's gate reads but does not wire it.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-B`,
  AC02/AC03), `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-09-30-f29-a-damage-graphs-hit-ordering-lifecycle.md`
  (the resolver, its channel routing and its follow-ups 5/6),
- `docs/findings/2026-10-02-f29-damage-zone-collider-call.md` (the F29-B/C
  collider slice already on main),
- `crates/cs_sim/src/damage/{resolver,graph}.rs`,
  `crates/cs_app/src/damage.rs`.
