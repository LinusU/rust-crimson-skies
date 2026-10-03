# F28-C.1: applying a guidance-loss detonation's declared blast

Date: 2026-10-03. Task: #546 "Apply a guidance-loss detonation's declared blast
and statuses at the item's last recorded position", a follow-up found while
implementing **F28-C** (task #122). Spec:
`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`, sections
`### F28-C` and `## Non-negotiable behavior` items 1, 3 and 4. Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md` ("Collision and ballistic tests",
"one physics pose owner"). Predecessors:
`docs/findings/2026-10-03-f28-b-ordnance-runtime.md` and
`docs/findings/2026-10-03-f28-c-ordnance-integration.md`. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no render, no audio, so no
`private/evidence/` report is produced and none is claimed.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/ordnance.rs` (extend): the new
  `GuidanceDetonation` record, `GuidanceTick::detonated` now carrying it,
  `guidance_tick` capturing the item's declared behavior and last recorded
  position before removing it, and the two new runtime methods
  `route_detonation` and `apply_detonation_statuses` (with the shared
  `apply_effects` body).
- `crates/cs_sim/src/weapons/mod.rs` (wiring only): the flat re-export list
  gains `GuidanceDetonation`.
- `crates/cs_app/src/ordnance.rs` (extend): `OrdnanceGuidanceBlast`,
  `OrdnanceSessionTick::guidance_blasts`, the `OrdnanceEventKind::GuidanceDetonated`
  network event, and the guidance branch of `step_ordnance_session` applying
  the blast.
- `crates/cs_app/tests/accept_f28_c_guidance_loss_detonation.rs` (**new**, one
  test), task test prefix `accept_f28_c_`.
- `crates/cs_sim/tests/accept_f28_b_ordnance_runtime.rs` (one assertion
  updated): `GuidanceTick::detonated` is now a list of records, so the F28-B
  test names `detonated.first().projectile()` instead of comparing to
  `vec![ProjectileId]`. The behavioral check is unchanged.
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary.

**One observable failure, before the change:** `guidance_tick` removed a
guided item whose target was lost with `LostTargetBehavior::Detonate` and
reported only its id in `GuidanceTick::detonated`; `step_ordnance_session`
then released the item's engagement and emitted `GuidanceDetonated`-less
nothing. A detonation therefore applied **no** declared damage and **no**
declared status effect, while a fuse-triggered item applied both. The minimum
scenario makes it concrete:
`accept_f28_c_a_guidance_loss_detonation_applies_its_declared_blast_at_the_last_position`
destroys a guided item's designated target, and a detonation that did not route
its declared channels or apply its declared status would have changed no
integrity and no engine status. Removing the blast application turns it red
(see sensitivity below).

## What was wired

The F28-B runtime already enforced the lost-target contract; this slice adds
the *consequence* the F28-C finding deferred:

1. **Capture.** `guidance_tick` builds a `GuidanceDetonation` for each
   `Lost { Detonate }` update **before** removing the item. The record carries
   the item's `ProjectileId`, shooter, the declared `ProjectileOrdnance` and
   the **last recorded position** (`LiveOrdnance::current`). The session step
   runs guidance before motion, so that is the position the item reached on the
   previous advance, not a fabricated impact point.
2. **Route.** `OrdnanceRuntime::route_detonation` mirrors `route_trigger`:
   the caller supplies the engagement's damage target and node, the runtime
   emits one `HitEvent` per non-zero declared channel stamped with the item's
   shooter and a fresh session hit id. The item's id is latched into the
   runtime's routed set, so a second call is `AlreadyRouted` — damage is
   applied once, structurally, exactly as for a trigger.
3. **Apply.** `OrdnanceRuntime::apply_detonation_statuses` mirrors
   `apply_statuses` for an item that is already gone: it bridges the record's
   declared effect list into the bounded `StatusEffectLedger` under the
   component's own `OrdnanceId`. Both methods share one `apply_effects` body,
   so the expiry rule and the source naming are stated once.
4. **Report and consume.** `step_ordnance_session` applies the blast through
   the shared `DamageResolver` and the ledger, records an `OrdnanceGuidanceBlast`
   in `OrdnanceSessionTick::guidance_blasts` (hits, per-channel damage, the
   authority's resolution or refusal, the applied status instances or the
   refusal) and appends a session-stamped `GuidanceDetonated` network event.
   A detonation with no recorded engagement is named by
   `OrdnanceRoutingRefusal::MissingEngagement`, like a trigger.

## Bounds (why no radius or falloff is invented)

- The blast uses the item's **declared** `ProjectileOrdnance`: its declared
  damage channels and its declared status effects, never a synthesized set.
- The declared `AreaEffect` travels with the record unchanged; no radius and
  no falloff is fabricated, and a component that declares no area effect
  declares none here either.
- The declared area may never outlive the item
  (`OrdnanceDefinitionError::AreaOutlivesItem`), and the item must still be
  live for a loss to be reported, so the blast is bounded by the declaration
  and by the item's `lifetime_ticks` together.
- The damage and status effects are the *same declared consequence a triggered
  item gets*: the routed hits go through the shared `DamageResolver` once and
  the statuses through the bounded ledger, with no second authority.

## Unknowns recorded (not guessed)

Everything numeric in the test is synthetic fixture data. The original facts
this slice deliberately does **not** decide — because they are unmeasured and
would be guesses — are filed for measurement as `create_tasks` follow-up
**#548** and repeated here:

- **What a lost-target `Detonate` blast damages** — the tracked actor's part
  (as this slice does through the engagement the producer named), every actor
  inside the declared radius, or something else. This slice keeps the
  engagement the producer supplied, because that is where a triggered item's
  effects already land and non-negotiable 1 forbids inventing a second rule.
- **The falloff inside the declared area effect** — whether damage scales with
  distance, and over which shape; no falloff is applied here.
- **Which channels the blast delivers** — this slice carries the item's own
  declared channels unchanged.
- **Whether arming gates a lost-target detonation** — the F28-B runtime does
  not consult the arming rule on a guidance loss; whether the original did is
  unknown.
- **Whether the original area effect damages, chokes, stalls or marks** — the
  same F28-A unknown, unchanged; this slice only carries whatever the
  declaration says.

These do not unblock the F28-C.1 behavioral claim: the claim is that a declared
`Detonate` loss now applies its **declared** consequence at its **last
recorded** position exactly once, not that the declaration matches the 2000
original.

## Test sensitivity (measured, one mutation at a time)

One mutation was applied to `crates/cs_app/src/ordnance.rs` and the F28-C.1
test binary re-run; the file was restored afterwards:

| mutation | caught by | result |
| --- | --- | --- |
| the guidance branch applies no blast (the per-detonation body is skipped) | `accept_f28_c_a_guidance_loss_detonation_applies_its_declared_blast_at_the_last_position` | 1 failed at `guidance_blasts.len() == 1` |

## Not claimed

No original-data verification, no claim that any ordnance family, guidance
rule, lost-target behavior, blast radius, falloff, damage channel or status
effect matches the 2000 original, no audio or visual verification, no Avian
body, and no network packet encoding. This slice awards at most **checked**;
F28-D, #548 and the owner's evidence gate the rest.
