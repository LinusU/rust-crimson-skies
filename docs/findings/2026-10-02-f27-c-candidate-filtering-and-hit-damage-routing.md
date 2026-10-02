# F27-C: where candidate filtering lives, and how a swept hit becomes damage

Date: 2026-10-02. Task: #443 "Decide the F27-C boundary between
`WeaponRules::eligible` and `Ballistics::sweep`"
(`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, section
`### F27-C`, in preparation for that stage). Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md`, "Collision and ballistic tests".
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no
render, no audio, so no `private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/guns.rs` (extend): `SweepCandidate`,
  `SweptContact`, `Ballistics::sweep_with_sources`,
  `WeaponRules::admit_candidates`, `WEAPON_DAMAGE_CHANNELS`,
  `RoutedHit`, `SweepOutcome`, `SweepRefusal` and `GunHitRouter` —
  plus `Ballistics::sweep`, now implemented as the same test with the source
  index dropped. Module docs updated.
- `crates/cs_sim/src/weapons/mod.rs` (wiring only): the flat re-export list
  and the module docs.
- `crates/cs_content/src/weapons.rs` (extend): `InteractionOption`,
  `InteractionRules::applied_by` / `deferred` / `is_known`,
  `APPLIED_BY_CANDIDATE_FILTER`, `DEFERRAL_STAGE`, and the
  `InteractionRules` docs that say which options are applied and which are
  deferred.
- `crates/cs_app/src/weapons.rs` (extend): `resolve_swept_damage` and
  `SweptDamageOutcome` — the production caller that hands the routed hits to
  the session's `DamageResolver`. `lower_rules`'s doc updated.
- `crates/cs_sim/tests/accept_f27_c_sweep_query_and_hit_routing.rs`
  (**new**, 11 tests), `crates/cs_content/tests/accept_f27_c_interaction_rule_deferral.rs`
  (**new**, 4 tests),
  `crates/cs_app/tests/accept_f27_c_weapon_damage_wiring.rs` (**new**, 4
  tests). Task test prefix: `accept_f27_c_`.
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary.

**One observable failure, before the change:** F27-A made the sweep find
contacts and F29 made `DamageResolver` the authority that applies them, but
**nothing connected them**. A round crossing a thin target produced a
`SweptHit { projectile, target, time_of_impact }` and stopped there: there was
no production code that turned one into a `cs_sim::damage::HitEvent`, so the
gun's declared per-channel damage amounts reached no part of any aircraft and
`DamageResolver::resolve` was never called from the weapon side. The
once-per-projectile guarantee had no damage to protect, and a zero-hit round
was indistinguishable from a hit whose damage had been dropped.

## The decision: three functions, in this order

F27-A left the seam open: `WeaponRules::eligible` filtered a candidate list
by *declared* policy, and `Ballistics::sweep` was pure geometry over an
already-filtered list. That split is kept, and the F27-C query stage is the
**third** thing, not a change to either.

| step | owner | what it decides | what it must not know |
| --- | --- | --- | --- |
| 1. admit | `WeaponRules::admit_candidates` (`cs_sim::weapons`) | which world candidates this gun's declared rules allow | geometry, damage, damage nodes |
| 2. sweep | `Ballistics::sweep_with_sources` (`cs_sim::weapons`) | which admitted boxes the segment crosses, in earliest-time-of-impact order, and which candidate each contact came from | allegiance, gun rules, damage amounts, `DamageChannel` |
| 3. route | `GunHitRouter::route` (`cs_sim::weapons`) | which `HitEvent`s each contact becomes, on which node, in which order | the damage graph's armor/overflow rules; whether the hits are applied |
| apply | `DamageResolver::resolve` (`cs_sim::damage`) | what the hits do to the graph | projectiles, gun banks, allegiance |

### Why the filter stays a rules query and not part of the geometry

* **The sweep must stay reusable and testable alone.** `FLIGHT-PHYSICS`
  requires a swept test over an already-filtered list: it names relative
  motion, earliest time of impact and stable tie-breakers, and says nothing
  about who may be hit. F27-A's AC01 drives it with a hand-built candidate
  list that has no allegiance at all. Putting the filter inside `sweep` would
  force every such caller to construct a relation table to say "hit this
  box", which is exactly the coupling the split avoided.
* **The sweep has no rule to consult.** `WeaponRules` is per-gun *declared*
  data. A bare `sweep(segment, targets)` call carries no rules argument, so a
  filter inside it would have to be a hardcoded hostility test — precisely the
  thing the declared rules exist to avoid, and the thing
  `accept_f27_c_the_declared_rules_filter_candidates_before_the_geometry`
  pins by widening the *declared* rule and watching the admission widen with
  it.
* **Self-hit and friendly fire are declarations, not geometry.** F27
  non-negotiable 4 requires them "by evidence or mark unknown", and the
  schema carries them `Resolved` so an unmeasured rule refuses to lower. A
  rule that can be unmeasured cannot be hardcoded into a numeric test.

### Why the routing is its own stage and not inside the damage layer

* `cs_sim::damage::DamageResolver` consumes **typed hits** (F29-A) and knows
  nothing about allegiance, gun banks, projectiles or `WeaponDamage`. Folding
  the routing into it would make the damage authority depend on the weapon
  model, so an F29-only change could break F27 and vice versa.
* The **node** a hit lands on is a property of the *collision feature* that
  reported the contact, not of the gun. That is why `SweepCandidate` carries
  `(box, node, relation)` and the routing takes the node from the candidate
  whose box the round reached **first**
  (`accept_f27_c_the_routed_node_is_the_part_the_round_reached_first`): a
  target's parts are separate candidates, the ledger keeps one contact per
  `(projectile, actor)`, and the winning candidate's node is the part
  actually reached. No part is chosen by proximity, by list order or by a
  default node, and none is invented — the original's part collision shapes
  are unmeasured (F27-D).
* Each channel is routed from the **gun definition's own amount**
  (`WeaponDamage`), with no multiplier and no invented penetration or ricochet
  behavior. The armor/interception rule stays where F29 put it: the resolver
  routes an `Armor`-channel hit through the node's declared armor zone.

### What `Ballistics::sweep` became

`sweep` is now `sweep_with_sources(...).into_iter().map(|c| c.hit)`. Same
geometry, same ordering, same ledger — one implementation, so the two cannot
drift. `sweep_with_sources` additionally reports the index of the candidate a
contact came from, which is what makes the routing possible **without a
second geometry test**; the same-actor duplicate collapse keeps the earliest
contact *and its source*, so several collision features reporting one contact
still produce one hit naming the part reached first.

## The three unapplied interaction options: deferred to F27-D

`WeaponRules { penetration, ricochet, ammo_switching }` are declared, typed
and lowered, and **no production code reads them**. That is now an explicit,
machine-readable deferral rather than an implicit gap:

| option | applied by | deferred to | why not applied |
| --- | --- | --- | --- |
| `self_hit` | `WeaponRules::admit_candidates` | — | — |
| `friendly_fire` | `WeaponRules::admit_candidates` | — | — |
| `penetration` | — | F27-D | a penetration model would have to invent what a round does *after* it passes through a part; F27 non-negotiable 4 forbids a simulator feature game content does not support, and the original ammunition's behavior is unmeasured |
| `ricochet` | — | F27-D | a ricochet model would have to invent the direction a round leaves a part in; the original's behavior is unmeasured, so a modeled reflection would be a guess presented as a rule |
| `ammo_switching` | — | F27-D | switching ammunition *type* in flight needs a per-mount inventory of several types and a selection rule over them; neither exists, and selecting a gun **bank** (`WeaponState::select`) is a different rule that must not be mistaken for it |

The deferral is **visible in the schema**, not only in this file:
`cs_content::weapons::InteractionOption` names each option,
`InteractionRules::applied_by` reports the production path that applies it,
and `InteractionRules::deferred` returns each unapplied option with its
resolving stage and reason. An audit asks the content contract; it does not
have to trust prose. `accept_f27_c_every_declared_option_is_either_applied_or_deferred`
fails if a sixth option is added without being reported one way or the other,
so the report cannot silently go stale.

`is_known` is reported **separately** from `applied_by`, because the two
questions differ: a production path consults the self-hit rule whether or not
this record knows what it is, and it is the *lowering boundary* that refuses
an unknown value. Conflating them would let a record claim a known rule it
never declared.

The runtime half carries the same note: `cs_sim::weapons::WeaponRules`'s
field docs and `cs_app::weapons::lower_rules` both point at the schema's
report, and the declared record stays the options' only home — dropping them
at the boundary would make the declared schema and the runtime disagree about
what a gun carries.

## A swept hit becomes a `HitEvent` (the acceptance criterion)

`GunHitRouter::route(shot, segment, candidates, rules, ballistics, at)`:

1. refuses a foreign session generation or a segment belonging to another
   projectile, **whole** — nothing is admitted and nothing is routed;
2. admits candidates through `WeaponRules::admit_candidates`;
3. sweeps the survivors with `sweep_with_sources`;
4. converts each contact into one `HitEvent` per **non-zero** declared damage
   channel, on the winning candidate's node, stamped with the router's own
   `HitEventId`.

Choices worth stating:

* **Zero-amount channels produce no hit.** A round that declares no internal
  damage does not put a zero-damage event in the damage stream; otherwise the
  resolver would have to treat "applied 0" and "never routed" as the same
  thing (`accept_f27_c_a_channel_with_no_declared_amount_produces_no_hit`).
* **Channels are not merged.** One hit per channel, because the resolver
  routes each through its own declared armor interception and overflow chain.
* **The routed hits are stamped with `at`, not `shot.id.tick`.** A projectile
  is fired on one tick and lands on a later one, and `DamageResolver::resolve`
  refuses a batch whose hits carry any other tick — stamping a landing with
  its muzzle's tick would make every travelling round unresolvable.
* **The producer serial is the routing system's, not the shooter's.** A
  `HitEventId` producer is a `u32` while an `ActorId` serial is never recycled
  within a session, so narrowing a shooter's serial into it could give two
  shooters the same producer (the same reason `FireEventId` widened its own
  producer to `u64` in F27-A).
* **The session generation crosses into `cs_types::net::SessionId` at the
  routing boundary, and generation zero is refused by name.** `main` unified
  the damage identity onto `cs_types::net` (task #442), so a routed
  `HitEventId` is the shared `EventId` whose `session` is a nonzero
  `SessionId`, while this module's own ids (`FireIntentId`,
  `FireEventId`, `ProjectileId`) still carry a `u64` generation. The router
  keeps the `u64` it shares with `FireResolver` and converts once, where the
  `HitEventId` is built. Zero cannot become a `SessionId` — it *is* "no
  session" in that type — so a router opened on generation zero is refused
  whole (`SweepRefusal::NoSession`) rather than panicking inside a tick. A
  real session never reaches that arm (`SessionAllocator` issues from 1, and
  an `ActorId` cannot be built for session 0 at all);
  `accept_f27_c_a_router_on_session_zero_refuses_whole` reaches it
  deliberately to prove it is a refusal and not a crash.
* **A refused contact is named, never dropped.** `SweepOutcome` carries the
  admitted candidates, the hits *and* the refusals, so an empty hit list is
  readable as "this round crossed nothing" rather than "damage was lost".
  `accept_f27_c_a_round_that_crosses_nothing_reports_a_miss` pins that
  distinction: a filtered-out candidate is a miss, not a refusal.
* **Retry is the ledger's job.** Re-routing the same segment produces no
  further hit, because `Ballistics` already holds `(projectile, actor)`
  (`accept_f27_c_a_retry_of_the_same_segment_applies_no_further_damage`). The
  router holds no other state, so teardown is dropping it and there is no
  retry path that has to unwind anything.
* **The routing applies nothing.** `cs_app::weapons::resolve_swept_damage` is
  the production caller that hands the routed `HitEvent`s to the session's
  `DamageResolver`; it is the only consumer, and its `SweptDamageOutcome`
  returns the routing outcome *and* the resolver's result (or its error) so a
  lost hit can never hide behind a successful resolution. It resolves the batch
  in a single call whether or not the round crossed anything, because
  `DamageResolver::resolve` answers an empty batch with an empty resolution for
  the tick and changes nothing: a caller reads the *sweep* to tell a miss from
  damage, never the emptiness of the resolution.
* **A part the damage graph does not declare is refused, not repaired.** The
  node comes from the candidate, so a collision feature can report a part the
  target's graph has no node for. The routing stamps it unchanged — inventing a
  fallback node would be exactly the proximity guess this stage refuses — and
  the authority refuses each hit by name
  (`DamageEventKind::HitRefused { reason: RefusalReason::UnknownNode }`),
  applying nothing (`accept_f27_c_a_candidate_node_the_graph_lacks_is_refused_by_name`).

### AC03 end to end through a lowered declared gun

`accept_f27_c_ac03_through_lowered_guns_switches_bank_mid_cooldown` (cs_app)
and `accept_f27_c_ac03_switching_bank_mid_cooldown_neither_duplicates_fire_nor_refills_ammo`
(cs_sim) run the whole acceptance criterion through two *declared* guns on
two mounts: the bank fires on tick 0, each round's swept hit becomes
`HitEvent`s carrying **that mount's** declared per-channel amounts and the
session's `DamageResolver` applies them; the bank is switched mid-cooldown
(selection refills and drains nothing); the next tick's shot is refused with
`FireDenialReason::Cooldown` and applies no damage; and the already-landed
projectile re-routed drains nothing further.

## Test sensitivity (measured, not asserted)

Six mutations were applied to the production code, one at a time, and the new
tests were re-run **twice** — once before the `GunHitRouter` took ownership of
its `Ballistics` ledger, and once after, so the table below is measured
against the signatures that were actually submitted. Every one was caught:

| mutation | caught by |
| --- | --- |
| `admit_candidates` filter disabled (`\|\| true`) | 4 tests, incl. `accept_f27_c_the_declared_rules_decide_admission_through_the_boundary`, `accept_f27_c_a_round_that_misses_reports_no_damage_resolution` |
| ledger filter removed from `sweep_with_sources` | `accept_f27_c_a_lowered_guns_swept_hit_applies_its_declared_damage`, `accept_f27_c_a_retry_of_the_same_segment_applies_no_further_damage`, `accept_f27_c_ac03_switching_bank_mid_cooldown_...` |
| internal channel never routed | 5 tests, incl. `accept_f27_c_a_swept_hit_becomes_one_hit_event_per_declared_channel` |
| node always taken from `admitted.first()` | `accept_f27_c_the_routed_node_is_the_part_the_round_reached_first` |
| schema reports all five options as applied | `accept_f27_c_penetration_ricochet_and_ammo_switching_are_deferred_to_f27_d`, `accept_f27_c_every_declared_option_is_either_applied_or_deferred` |
| `resolve_swept_damage` never calls `damage.resolve` | `accept_f27_c_a_lowered_guns_swept_hit_applies_its_declared_damage`, `accept_f27_c_ac03_through_lowered_guns_switches_bank_mid_cooldown` |

The reviewing agent re-measured all six mutations independently on the reviewed
head, one at a time, with
`cargo test --no-fail-fast -p cs_sim -p cs_content -p cs_app -- accept_f27_c_`
so that every test binary runs even after one fails (plain `cargo test` stops at
the first failing binary and under-reports). All six were caught, and two counts
in the original table were wrong and are corrected above: the disabled rules
filter is caught by 4 tests, not 2, and marking every option applied is caught
by 2 of the 4 deferral tests, not all 4 (the other two assert on values that
mutation does not change). The review also measured a seventh, unlisted
mutation — replacing the router's session ledger with a fresh `Ballistics` per
pass — which the retry guarantees catch (3 tests).

## Review notes (2026-10-02, reviewing agent bunny-2)

The decision above stands as recorded. The review changed four things, none of
which alters the boundary:

1. `cs_sim::weapons::WeaponRules`'s field docs pointed at a reporting API that
   does not exist (`cs_content::weapons::InteractionOption::use_of` and an
   `OptionUse` enum with `Consumed`/`Deferred` variants). The shipped report is
   `InteractionOption::applied_by` plus `InteractionOption::deferred_to`, and
   the docs now name that. A finding must not describe an API that was never
   built.
2. `cs_app::weapons::resolve_swept_damage` short-circuited an empty routed
   batch and hand-built an empty `TickResolution` instead of calling the
   resolver, with a comment claiming the resolver would have answered
   differently. It does not: `DamageResolver::resolve` returns
   `TickResolution { tick, events: [] }` for an empty batch and advances no
   state, so the branch and its stated reason were both redundant. The single
   call remains, so there is still exactly one path from a routed hit to the
   authority.
3. Added `accept_f27_c_a_candidate_node_the_graph_lacks_is_refused_by_name`
   (cs_sim, 11 tests there now), pinning the error path where a collision
   feature reports a part the target's damage graph does not declare: the hit
   is refused by name and nothing is applied.
4. Corrected the two sensitivity counts above, which the review measured
   rather than inherited.

## Unknowns recorded (not guessed)

- **Which part of a target a round damages, in the original.** The routing
  takes the node from the collision feature's candidate, because the part
  geometry is unmeasured. F27-B owns the hierarchy/part-geometry walk that
  produces those candidates; F27-D measures whether the original's per-part
  mapping matched anything.
- **Whether the original's self-hit and friendly-fire rules are what the
  fixture declares** (`Excluded`, `HostileOnly`). The rules are applied
  exactly as declared, and the declaration is designed, not measured.
- **Penetration, ricochet and ammunition switching** — the whole of their
  semantics, deferred to F27-D as above.
- **The original's damage numbers.** Every amount in these tests is the
  synthetic fixture's own, carried unchanged from the declared record.

## Not claimed

No original-data verification, no ECS system, no schedule wiring, no audio or
muzzle-effect consumer, no cockpit bank-selection input, no projectile body or
Avian collider, and no in-flight ammunition switching. This stage closes the
*swept hit → `HitEvent`* seam and makes the deferral explicit; F27-B still
owns the cadence loop, the live mount transforms and the part geometry that
feeds `SweepCandidate`, and F27-C's application half still owns the ECS
wiring, the effects/audio consumers and the selection input. The task awards
at most **checked** status.