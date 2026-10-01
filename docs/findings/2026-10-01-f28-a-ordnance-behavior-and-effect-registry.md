# F28-A: Ordnance behavior, effect registry, proximity fuse, guidance loss, status effects and nitro

Date: 2026-10-01. Task: F28-A "Define exhaustive ordnance behavior and effect
registry" (`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
section `### F28-A`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`
("Boost and special models", "Collision and ballistic tests").
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/ordnance.rs` (new, ~3.9 k lines): the runtime
  contract — `OrdnanceId`, `OrdnanceFamily`, `HardpointKind`,
  `LaunchGeometry`, `StackLoad`, `ArmingRule`, `ProximityFuse`/`FuseRule`,
  `GuidanceRule`/`LostTargetBehavior`, `AreaEffect`, `StatusEffectKind`/
  `StatusEffectTarget`/`OrdnanceStatusEffect`, `OrdnanceMedia`,
  `EquipmentRules`/`CompatibilityVerdict`, `ProjectileOrdnance`, the nitro
  half (`NitroActivationRule`, `NitroTradeoffs`, `NitroParameters`,
  `NitroOrdnance`), `OrdnanceComponent`, `OrdnanceRegistry`, the proximity
  geometry (`TargetPath`, `ClosestApproach`, `closest_approach`), the in-flight
  `OrdnanceState` with its fuse decision, `GuidanceTracker`/`GuidanceSet`/
  `TargetObservation`, `StatusEffectLedger`, `NitroLedger`/`NitroTick`, and
  the synthetic fixture (`synthetic_registry`, `synthetic_proximity_flak`,
  `synthetic_*`, `SYNTHETIC_*`).
- `crates/cs_sim/src/weapons/mod.rs`, `crates/cs_content/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): the module declarations and the
  flat re-export/doc-comment lists. No logic.
- `crates/cs_content/src/ordnance.rs` (new): the declared,
  provenance-carrying schema — `DeclaredOrdnance`, `DeclaredOrdnanceDetails`
  (`DeclaredProjectile` / `DeclaredNitro`), `DeclaredOrdnanceFamily`,
  `DeclaredLaunchGeometry`, `DeclaredStackLoad`, `DeclaredArmingRule`,
  `DeclaredFuseRule`/`DeclaredProximityFuse`,
  `DeclaredGuidanceRule`/`DeclaredLostTargetBehavior`, `DeclaredAreaEffect`,
  `DeclaredStatusEffect`, `DeclaredOrdnanceMedia`, `DeclaredEquipmentRules`,
  `OrdnanceSchemaError` validation and the `declared_synthetic_*` fixtures.
- `crates/cs_app/src/ordnance.rs` (new): `lower_ordnance`,
  `lower_equipment_rules`, `OrdnanceLowerError`, `declared_scene_binding` and
  the generation-stamped `OrdnanceLauncherBinding` ECS record.
- `crates/cs_sim/tests/accept_f28_a_ordnance.rs`,
  `crates/cs_content/tests/accept_f28_a_ordnance_schema.rs`,
  `crates/cs_app/tests/accept_f28_a_ordnance_boundary.rs`: the 57
  `accept_f28_a_*` acceptance tests.
- This file.

**One observable failure:** with an endpoint-only proximity test, a shell
crossing a target's path at 210 m/s (7 m per tick at 30 Hz) against a
declared 12 m trigger radius is **silently missed**: both of the shell's
tick endpoints are further from the target's path than the radius, and the
crossing happens strictly inside the tick.
`accept_f28_a_proximity_fuse_triggers_for_a_fast_near_pass` asserts that both
endpoints are outside the radius, that the swept segment nevertheless comes
within it at `t = 0.5`, and that the armed shell detonates on that
near-pass; `accept_f28_a_proximity_fuse_does_not_trigger_before_arming`
asserts the other half — the identical geometry does nothing on every tick
before the declared arming delay. Replacing the swept distance with an
endpoint test, or dropping the arming gate, makes one of the two fail.

## Semantics defined at this stage

- **The registry is exhaustive with respect to the declared vocabulary, not
  to the installation.** `OrdnanceRegistry` holds one entry per catalog id,
  refuses a duplicate id at registration rather than letting the later entry
  win, and exposes `families()` — the audit rows F28-D walks against the
  original installation. Whether the registry is *complete* with respect to
  the original is F28-D's catalogue audit; nothing here claims it.
- **A component is a launched item or a booster, never a blend.**
  `OrdnanceComponent::{Projectile, Nitro}` keeps a booster with no fuse, no
  lifetime, no blast and no launch geometry. Forcing nitro into the
  projectile record would invent a detonation the original does not have and
  would hide the capacity arithmetic AC04 is about. Both arms are boxed
  (clippy's `large_enum_variant`): either record on its own is large enough
  that carrying it inline would make every registry entry as big as the
  larger of the two.
- **Nitro is a first-class PC feature, not an Xbox import.**
  Non-negotiable 2 requires it: the manual lists a nitro activation control,
  so `NitroOrdnance` is a registry entry with capacity, consumption,
  recovery, extra thrust, an activation rule and declared tradeoffs. The
  *numbers* are unknown, which is why every one is a `Resolved` value the
  boundary refuses to guess.
- **Non-negotiable 1 is executable, not a comment.** `check_family_rules`
  refuses four incoherent combinations, each because the contradiction is in
  the family's own name rather than in a guess about the original: a
  `DirectExplosive` or `ProximityFlak` may not be declared as a seeker; a
  `GuidedRocket` may not be declared unguided; an `AreaDenialEngine` may be
  neither targeted nor impact-fused (its effect is the bounded area, so a
  contact end would mean the area never happened); and a `NitroBooster` may
  not carry a fuse, a lifetime and a guidance rule. An `AerialTorpedo` is
  deliberately *unconstrained* — whether the original's torpedo was a seeker
  is exactly the kind of fact F28-D has to measure rather than assume.
- **The proximity fuse is a swept relative distance, gated by a separate
  declared arming rule.** `closest_approach` subtracts the target's own
  motion from the item's and reduces the pair to a segment-versus-segment
  distance, which is exact for two linear motions and cannot miss a pass
  that happens inside the tick. `OrdnanceState::fuse_decision` checks arming
  **first** and returns `FuseInert::NotArmed { ticks_live, travelled_m }` —
  so "not armed yet" and "armed but nothing in range" are distinguishable
  states, and a caller can see how much of the arming condition is
  outstanding. The degenerate zero-relative-displacement case is tested with
  `== 0.0` exactly rather than an epsilon, per `FLIGHT-PHYSICS`: the epsilon
  exists to avoid singularities, not to invent motion.
- **One item detonates once.** `fuse_decision` latches its trigger, so a
  second query for the same item reports `FuseInert::AlreadyTriggered` and
  names nothing new — the case `FLIGHT-PHYSICS` calls out ("apply damage
  once even if several collision features report the same hit") when two
  targets, or a proximity sweep and a contact sweep, both report the same
  item on one tick. Among several in-range targets the **nearest** wins with
  `ActorId` as the stable tie-breaker, so the answer does not depend on the
  order the caller collected candidates in.
- **Guidance loss is terminal and reported once.** `GuidanceTracker::lose`
  records the cause, drops the target and never re-acquires it; a later
  `lose` returns only the continuing consequence (`Coast`, `Disarmed`, or
  `Disarmed` for an item that already detonated), so a destroyed target is
  not announced on every tick. Every *targeted* rule names its
  `LostTargetBehavior`, so there is no "keep tracking" default. A session
  change is a loss (`LostTargetReason::ForeignSession`) applied by
  `GuidanceSet::session_tick`, not a lookup that might match an id in the
  next generation's roster: that is AC02's session half made structural
  rather than a convention every caller has to remember.
- **Area effects are bounded, and the bound is enforced.** An
  `AreaEffect`'s own lifetime may not exceed the item's, and a `Timed`
  fuse's ticks may not exceed the item's lifetime either — both refused by
  name rather than clamped. `StatusEffectKind` keeps damage, choke, stall and
  marker as four kinds rather than one "effect level".
- **Status recipients are stable ids.** A recipient is an `ActorId` plus an
  optional `SystemKind`, so the engine's choke and the aircraft's own stall
  are separate records even on one frame; `is_same_recipient` compares both
  halves, so an actor-wide choke never satisfies a query about the engine's.
  An effect's expiry tick is `tick + duration_ticks` in whole ticks and is
  reported exactly once: `advance_to` removes and returns what reached its
  boundary, and advancing to the current tick is a no-op, so a re-entered
  schedule step cannot re-apply an expiry. Time never runs backwards, and a
  foreign session is refused in both `apply` and `advance_to` — the restart
  boundary (AC03's second half).
- **Media is presentation only.** `OrdnanceMedia { visual, sound, particles }`
  is where the particle lives; no fuse, ledger or decision record carries a
  media field, so the separation non-negotiable 3 requires is structural
  rather than a convention. A component whose particle is missing loses a
  picture, never a detonation.
- **AC04's nitro shape is structural.** `NitroTick` carries the tick, the
  activation flag, `extra_thrust_n`, `consumed_units`, the authority
  multiplier and the refusal — and **no** pose, velocity or duration field.
  No method on `NitroLedger` takes a `std::time::Duration`: capacity is
  converted through the declared `TickRate` only. A caller cannot express a
  render-frame-scaled burn. Consumption covers *every elapsed tick*, so
  ten ticks walked and ten ticks jumped reach the same capacity; recovery
  applies only to an idle tick, so a held control does not drift upward.
  A refused activation consumes **nothing**
  (`NitroRefusal::CapacityExhausted`), which is the `FLIGHT-PHYSICS` rule
  "pressing a button while boost is unavailable does not consume capacity".
- **The nitro tradeoff is a slot, not a guess.** `NitroTradeoffs::UNMEASURED`
  is an authority multiplier of `1.0` with the meaning "no tradeoff has been
  measured". The field exists so a measured value can be installed later
  without changing a consumer; the synthetic fixture uses the unmeasured one
  and a test pins that, so a future agent cannot quietly introduce an
  invented authority penalty.
- **Equipment compatibility is one shared verdict.** `EquipmentRules` is a
  pure check, so the shop (F44-A, which depends on this task) and an import
  read the same answer instead of each inventing one. `requires` is checked
  before `forbids` so a verdict names one cause in a stable order.
- **An import cannot bypass the shop through a hole.**
  `OrdnanceRegistry::resolve_installation` refuses the whole loadout the
  moment one id is unknown, and refuses a repeated id the same way. A
  custom plane naming an equipment id the registry does not have is
  **refused**, not skipped: a loadout is not usable with a hole in it.
- **A launcher mount is the F29 weapon-mount node.** `LaunchGeometry::mount`
  is a `DamageNodeKey`, the same identity F27's `GunDefinition::mount` uses,
  so the node a destroyed weapon-mount node disables is literally the
  launcher that stops firing. The launch **pose** is a supplied
  `MountTransform`, and the world velocity is composed from the *declared*
  `InheritanceRule` through F27's own `MountTransform::world_velocity_mps`
  rather than a second composition rule. No launch offset, no convergence
  distance and no center-screen origin is invented.
- **The declared and runtime vocabularies stay separate.** `cs_sim` may not
  depend on `cs_content` (`docs/01-ARCHITECTURE.md`), so
  `cs_content::ordnance` keeps its own `DeclaredOrdnanceFamily`,
  `DeclaredArmingRule`, `DeclaredFuseRule`, `DeclaredGuidanceRule`,
  `DeclaredLostTargetBehavior`, `DeclaredStatusEffectKind`,
  `DeclaredHardpointKind` and `DeclaredNitroActivationRule`, and
  `cs_app::ordnance` maps them variant-wise. `WeaponDamage` and
  `DamageNodeKey` are the two *shared* records, as they are for F27.
- **Every load-bearing declared value is a `Resolved`,** and the boundary
  refuses each unknown **by field name** with its claim and reason — 16
  table-driven refusal paths in the two boundary tests (10 projectile fields,
  6 nitro fields) plus the inner fixed-burn tick and the two equipment
  options. Repairing an unknown into a plausible number is what F14
  non-negotiable behavior 3 forbids, and the tests pin it.

## Designed vocabulary, not original data

Everything load-bearing here is newly authored project design carrying
`Origin::SyntheticFixture` and designed provenance. **All** of the following
are unmeasured: the original PC ordnance catalogue and which of the six
families it actually has; the hardpoint layout; the launch speeds, release
delays and inherited-velocity shares; the arming delays and travel
distances; the fuse trigger radii and fuse *shapes*; the guidance rules and
lost-target behaviors; the lifetimes; the area radii and area lifetimes; the
damage numbers; the status effect kinds, durations and strengths; the media
resources; and the nitro capacity, consumption rate, recovery rate, extra
thrust, activation rule, burn duration and tradeoffs. F28's "Research
boundary" says the public manual establishes no ordnance table, and this
stage had no `CS_GAME_DIR` at all. Nothing here claims `verified_original`;
the catalogue audit is F28-D.

## Unknowns recorded (not guessed)

- **The original ordnance catalogue.** Which of direct explosive,
  proximity/flak, guided/tagged-target, area-denial/engine effect, aerial
  torpedo and nitro the PC installation has, and what each one is called in
  its data. `OrdnanceFamily` is a designed vocabulary of six leads, not a
  catalogue. F28-D enumerates the installation and maps every id to a family
  and a declared behavior.
- **The proximity fuse's trigger *shape*.** This stage declares one radius
  and tests the swept **center** path against it. A spherical fuse, a shaped
  zone and a directional sensor are three different original rules with
  different answers; `FLIGHT-PHYSICS` requires "a swept center/shape
  appropriate to the original rule", and which one applies is unmeasured.
  The `TargetPath` doc records this; F28-D has to say which.
- **Arming conditions.** Whether the original arms on elapsed time, on
  distance flown, on a launch-vehicle condition or on a target lock is
  unknown. `ArmingRule` has a `Disarmed` arm and two time/distance variants
  and **no** lock variant, because adding one would be inventing a rule the
  evidence does not support. F28-D either confirms a variant or adds a
  measured one.
- **Whether the original's torpedoes are guided.** Left unconstrained for
  exactly this reason.
- **What "lost target" does** in the original: detonate, coast or go inert.
  All three are declared, none is chosen by default, and F28-D records which
  each weapon uses.
- **Whether a destroyed target's loss is per-weapon or per-swarm**, and
  whether a tagged target can be re-acquired after a temporary loss. This
  stage makes loss terminal per item; a re-acquisition rule would be a
  different contract.
- **The nitro activation rule** (held control or fixed burn), the burn
  duration, the recovery model, and the original's tradeoffs if it has any.
- **Whether nitro is per-airframe or per-loadout, and whether capacity
  carries across a mission.** `NitroLedger` is per actor in one session and
  starts full.
- **Whether the original's area effects damage, choke, stall, mark or any
  combination**, and whether the combination is data or code. Four kinds are
  declared separately and a component may carry several; which combination
  the original uses is unmeasured.
- **Stack mass and capacity units.** `StackLoad` is capacity in units and
  mass in kilograms because that is what the F24 loadout-mass record and the
  shop's ammo count need; the original's own units and whether stack mass
  feeds a real mass change are F44/F26 questions.

## Test sensitivity

Sixteen distinct mutation probes were run against the implementation as
submitted, each breaking at least one named test (the review pass added three
more; see "Review record" below):

| Removed or broken | Test that fails |
| --- | --- |
| the arming gate in `fuse_decision` | `accept_f28_a_proximity_fuse_does_not_trigger_before_arming` |
| the target displacement in the relative-motion subtraction | `accept_f28_a_the_proximity_sweep_follows_relative_target_motion` |
| the swept distance (endpoint-only variant) | `accept_f28_a_proximity_fuse_triggers_for_a_fast_near_pass`, plus 3 more |
| the trigger latch | `accept_f28_a_one_item_detonates_once_however_many_contacts_report_it` |
| the status expiry boundary (one tick late) | `accept_f28_a_a_timed_engine_status_effect_expires_on_exactly_its_tick` |
| per-tick consumption (consumed one tick per jump) | `accept_f28_a_nitro_consumption_is_whole_ticks_not_frame_time` |
| the unknown-installation refusal (skip instead) | `accept_f28_a_an_unsupported_installation_is_refused_rather_than_skipped` |
| lost-target terminality | `accept_f28_a_each_lost_target_behavior_has_its_own_outcome` |
| the session-change loss | `accept_f28_a_guidance_loses_its_target_on_a_session_change` |
| the family-coherence rule | `accept_f28_a_no_family_may_declare_rules_that_contradict_it` |
| the unavailable-boost refusal | `accept_f28_a_pressing_nitro_without_capacity_consumes_nothing` |
| the boundary defaulting an unknown trigger radius | `accept_f28_a_each_unresolved_field_is_refused_by_name` |
| the boundary dropping an unresolved prohibition | `accept_f28_a_each_unresolved_equipment_option_is_refused_independently` |
| the boundary forcing an unknown tradeoff to `UNMEASURED` | `accept_f28_a_each_unresolved_nitro_field_is_refused_by_name` |
| the family/details agreement in the declared schema | `accept_f28_a_a_family_must_agree_with_its_declared_details` |
| the declared schema repairing an unknown into a known | `accept_f28_a_an_unknown_field_stays_unknown_in_the_declared_record` |

The last two matter most: an "unknown becomes a plausible number" boundary
is the failure this whole layer exists to prevent, and both halves of it are
now pinned by a named test.

## Not claimed

No original-data verification, no catalogue audit, no per-tick ordnance
system, no Avian body or collider, no ECS wiring of the runtime ledgers, no
routing of a fuse trigger into `cs_sim::damage::HitEvent`, no status-effect
consumer in the flight model, no nitro consumer of
`cs_sim::flight::BoostParameters`, no cockpit launch gesture, no hardpoint
firing order, no loadout or shop validation (F44 consumes `EquipmentRules`),
and no media decoding. F28-B implements the direct/proximity/guidance/status/
boost mechanisms; F28-C wires hardpoints, UI, AI use and network events;
F28-D closes the original catalogue and collects the evidence.

`NitroLedger` deliberately does not feed `flight::BoostParameters` at this
stage: the F24-A tuning record's boost parameters are a *design* record with
its own thrust and consumption terms, and connecting the two is a design
decision F28-B/F28-C should make explicitly rather than one this stage should
make silently.
## Review record (added by the review pass)

**Reviewer:** `bunny-alpha-1`, the same agent instance that implemented the
stage, in a session with no memory of the implementation. This review is
therefore **not independent evidence** (AGENTS.md, "Reviewing"); nothing here
is raised above `checked`, and the owner's own review is still outstanding.

Five defects were found and fixed, all inside the owner paths. Each is
pinned by a named `accept_f28_a_*` test that fails against the pre-review
code: the mutation was re-applied and re-run to confirm, not assumed.

1. **A held control interrupted an accepted fixed nitro burn, and paid out
   recovery while it did.** `NitroLedger::request` computed
   `active = (burn_running || requested) && refused.is_none()`, so on any tick
   of a `FixedTicks` burn where the control was still held the burn was
   refused (`BurnAlreadyRunning`) and therefore *inactive*. Because recovery
   is idle-only, those ticks then **regenerated** capacity at
   `recovery_per_s` while the pilot was boosting — with these numbers, more
   than was being spent. `NitroActivationRule::FixedTicks` promises "each
   accepted activation runs for this many whole ticks", so `active` is now
   `burn_running || (requested && refused.is_none())`: the refusal is about
   starting *another* activation, never about the one already accepted.
   Pinned by `accept_f28_a_a_fixed_nitro_burn_lasts_exactly_its_declared_ticks`
   and `accept_f28_a_a_running_fixed_nitro_burn_never_recovers_capacity`. The
   pre-review fixed-burn test asserted only the refusal, never the running
   burn's activity, so the defect was invisible to it; both halves are now
   asserted.
2. **A targeted item launched with no target reported a fabricated cause and
   the wrong behavior.** `GuidanceTracker::hold` was `&self`, and its
   `(lost: None, target: None)` arm returned
   `Lost { reason: Despawned, behavior: Coast }`, built from
   `GuidanceRule::Unguided.lost_target().unwrap_or(Coast)` — a value that is
   always `Coast` — for a rule that could have declared `Disarm` or
   `Detonate`. It also re-announced that loss on every tick, which is exactly
   what the module promises never happens. `hold` is now `&mut self` and
   routes the case through `lose` once, and the cause is a new
   `LostTargetReason::Unassigned`: an item that never had a target did not
   lose one. `GuidanceSet::session_tick` no longer turns a target-less
   tracker into a `Despawned` loss either. Pinned by
   `accept_f28_a_a_targeted_item_launched_without_a_target_reports_one_unassigned_loss`.
3. **`GuidanceError` had no producer.** The type was exported but nothing
   could return it, so `GuidanceSet` could only answer `Option` and an
   unregistered item was silently *skipped* — the failure mode
   `OrdnanceRegistry::require` exists to prevent. `GuidanceSet::require` now
   mirrors it, pinned by
   `accept_f28_a_guidance_refuses_an_unregistered_item_rather_than_skipping_it`.
4. **The declared schema reported a corrupt damage amount as a corrupt status
   strength**, without saying which channel: `armor_damage` and
   `internal_damage` raised `NonFiniteStatusStrength` /
   `NegativeStatusStrength`. `OrdnanceSchemaError` now carries
   `NonFiniteDamage { field }` and `NegativeDamage { field, damage }`.
5. **`validate_nitro` named no field at all**, reporting `field: "nitro"` for
   all four numbers ("the declared nitro nitro is unusable") even though the
   variant documents `field` as the corrupt field. It now names
   `capacity_units`, `consumption_per_s`, `recovery_per_s` or
   `extra_thrust_n`. Pinned by
   `accept_f28_a_a_corrupt_declared_field_is_named_in_its_refusal`.

Three documentation defects in this file were also corrected: the probe count
said "fifteen" over a sixteen-row table, the refusal-path arithmetic summed
to 19 under a heading of 16, and `crates/cs_sim/src/lib.rs` was listed as
wiring although it was not touched. `fuse_decision`'s doc now states the
`impacts` ordering it assumes (`Ballistics::sweep`'s ascending time of
impact) and the one consequence of the arming/proximity/lifetime order F28-B
needs to know: an expired proximity item still detonates on an in-range path,
and reports `OutOfRange` rather than `Expired` when paths are presented but
none is in range. Which rule the original used is unmeasured, so the behavior
was documented, not changed.

## Still open for F28-B (unchanged by the review)

- Whether an item whose lifetime has run out may still trigger its proximity
  fuse on a path inside the declared radius. The order in `fuse_decision`
  answers "yes", and that answer is **unmeasured**.
- The original's fuse trigger *shape* (sphere, shaped zone, directional
  sensor). Only the radius is declared and only the radius lowers.
- Whether a fixed nitro burn should be interruptible, and whether capacity
  carries across a mission.
