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
- `crates/cs_sim/src/weapons/mod.rs`, `crates/cs_sim/src/lib.rs`,
  `crates/cs_content/src/lib.rs`, `crates/cs_app/src/lib.rs` (wiring only):
  the module declarations and the flat re-export/doc-comment lists. No logic.
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
  render-frame-scaled burn burn. Consumption covers *every elapsed tick*, so
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
  refuses each unknown **by field name** with its claim and reason — 16 named
  refusal paths in the two boundary tests (10 projectile fields, 6 nitro
  fields, 2 equipment options and the inner fixed-burn tick). Repairing an
  unknown into a plausible number is what F14 non-negotiable behavior 3
  forbids, and the tests pin it.

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

Fifteen distinct mutation probes were run against the implementation, each
breaking at least one named test:

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