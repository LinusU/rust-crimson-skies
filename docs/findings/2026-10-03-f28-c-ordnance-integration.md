# F28-C: the ordnance session, its per-tick step, the ECS mirror and the network events

Date: 2026-10-03. Task: #122 "Integrate hardpoints, UI, AI use and network
events" (`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
section `### F28-C`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`
("Boost and special models", "Collision and ballistic tests"). Predecessors:
`docs/findings/2026-10-01-f28-a-ordnance-behavior-and-effect-registry.md` and
`docs/findings/2026-10-03-f28-b-ordnance-runtime.md`. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no render, no audio, so no
`private/evidence/` report is produced and none is claimed.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/ordnance.rs` (extend, this stage's whole change): the
  [`OrdnanceSession`] (ownership, registration, `register`, `engine_status`,
  `close`), [`step_ordnance_session`], [`live_launcher_transforms`],
  [`OrdnanceItemMirror`], [`sync_ordnance_mirrors`], `RegisteredOrdnance`,
  `OrdnanceRegistrationError`, `OrdnanceEffect`, `OrdnanceEventKind`,
  `OrdnanceNetworkEvent`, `OrdnanceLaunchId`, `OrdnanceEngagement`,
  `OrdnanceLaunch`, `OrdnanceOrder`, `OrdnanceOrderRefusal`,
  `OrdnanceStepRefusal`, `UnreadableLauncher`, `OrdnanceDetonation`,
  `OrdnanceRoutingRefusal`, `OrdnanceMirrorReport`, `OrdnanceTeardownReport`,
  `OrdnanceSessionTick`, `OrdnanceStep` and `EngineStatus`. The only edited
  lines are the import block; the F28-A lowering and binding are unchanged.
- `crates/cs_app/tests/accept_f28_c_ordnance_session_step.rs` (**new**, 12
  tests), task test prefix `accept_f28_c_`.
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary. The `crates/cs_app/src/lib.rs` module declaration already existed from
F28-A, so no wiring edit was needed there.

**One observable failure, before the change:** F28-A lowered and bound the
declared components and F28-B owned the per-tick runtime, but **no production
code ever called the runtime in sequence**. Nothing turned a producer's launch
request into a live item — the launch mount pose existed only as a declared
`DamageNodeKey`, with no walk from the live ECS hierarchy; nothing applied a
triggered item's declared damage to the shared `DamageResolver`; nothing
applied its declared status effects to a stable recipient; nothing turned an
accepted launch or a status transition into a network event; nothing mirrored
an authoritative in-flight item into the ECS; and there was no session whose
teardown released the items, the status ledger and the nitro tables together.
The minimum scenario makes it concrete: a declared area-denial item could be
lowered, launched into an `OrdnanceRuntime` and have its choke applied by
hand, but no per-tick path existed that triggered it, applied the effect,
reported the expiry on its exact tick and then started a clean session.
`accept_f28_c_a_timed_engine_status_expires_on_its_tick_and_resets_on_restart`
drives that whole path and turns red if the advance, the status application or
the expiry handling is removed.

## What this stage wires

`step_ordnance_session` is the one path that turns a tick's ordnance orders
into the whole pipeline, in this order:

1. **ownership gate** — one session generation owns the `OrdnanceRuntime`, the
   lowered components, the engagement records, the once-only launch-id ledger,
   the launch-effect log and the network-event log. A tick that is not strictly
   after the last one resolved, or any order after teardown, is refused whole
   and changes nothing.
2. **status expiry** — `advance_status(step.at)` runs first so an effect whose
   boundary is this tick is reported once, on its own tick, and cannot be
   resurrected by a re-entered step.
3. **orders** — `OrdnanceOrder::Launch` resolves the named component against
   the session's lowered registry, reads that actor's live launcher poses
   through [`live_launcher_transforms`], allocates the item's id itself (so a
   network replay can never name a second item), launches through the runtime,
   records the engagement, emits the declared media effect and appends a
   stamped event. `OrdnanceOrder::Nitro` drives the actor's booster and appends
   an event for an accepted activation. Each order changes nothing on refusal
   and is named by its own refusal variant.
4. **guidance** — `guidance_tick` drives the lost-target contract when an
   observation is supplied; an item that ended by losing its target has its
   engagement released with it.
5. **motion** — `advance` moves every live item and reports its swept segment,
   its lifetime expiry and the items removed because their fuse had already
   triggered. The tick is all-or-nothing inside the runtime.
6. **fuse decisions and the consumer** — every item still live after the
   advance runs its own `OrdnanceState::fuse_decision` (arming gate, swept
   proximity and once-only latch unchanged). A triggered item routes its
   declared channels once into the shared [`DamageResolver`] and applies its
   declared status effects to the engagement's stable `StatusEffectTarget`,
   emitting the matching events.
7. **retirement and mirror** — a retired or expired item releases its
   engagement; `sync_ordnance_mirrors` reconciles the ECS: every live item gets
   an [`OrdnanceItemMirror`] entity whose `Transform` is *written* from the
   authoritative runtime position, and a mirror with no live item — retired,
   expired, or left by a reloaded scene — is despawned.

Teardown is [`OrdnanceSession::close`]: it releases every live item, despawns
every mirror and **drops and rebuilds the runtime empty**, so the status
ledger, the nitro tables and the routed ledger cannot be read out of a session
that no longer exists.

## Ownership, authority and what is deliberately not here

- **One authority, one session.** The session owns the `OrdnanceRuntime`; the
  `DamageResolver` is *borrowed* from the caller, because F29 owns that
  authority's lifecycle and the collision/crash producers share it. Nothing
  here applies damage outside it.
- **The producer is an order, not a cockpit and not an AI.** `OrdnanceOrder`
  is the input boundary the cockpit and the AI share. The original's hardpoint
  firing order, launch control, ammo cycling and the AI's launch decision are
  unmeasured; the dependent tasks (F46-B HUD/input, F32-B AI firing solutions,
  F44-B loadout validation) own them. This is the same boundary F27-C drew for
  `WeaponOrder::SelectBank`.
- **Network events are an in-memory stamped log, not packets.**
  `OrdnanceNetworkEvent` carries a session `EventId` (session, tick, producer,
  sequence) for a later encoder; this stage neither serialises nor transmits.
  F57-B owns the networking boundary.
- **The ECS mirror is a mirror.** Its `Transform` is written from the
  authoritative position every tick and carries no collider, so Avian never
  integrates an ordnance item and no second contact authority exists
  (FLIGHT-PHYSICS, "one physics pose owner").
- **`EngineStatus` is the flight-model consumer interface.** `is_under` and
  `engine_status` turn the choke/stall/damage/marker ledger into the value the
  flight model will read; wiring that value into `cs_sim::flight` is task #453
  (already queued), not guessed here.
- **Not in this stage**: scheduling the step inside the session run loop
  (`crates/cs_app/src/run.rs` is not an owner path), an Avian body/collider for
  an item, audio-device playback or rendering of the emitted effects, and the
  guidance-loss blast application described below.

## Choices worth stating

- **The launcher walk is local.** `crate::weapons` is not an owner path for
  F28-C, so this module reuses its public records (`MountPoseBinding`,
  `MountPoseRefusal`, `LiveMountTransforms`) and re-reads the ancestor chain
  itself. A mount whose binding is stale, has no pose, a non-finite origin, a
  zero forward axis or no reachable airframe velocity is refused **by name**,
  and the launch that needed it is refused rather than firing from the world
  origin.
- **The session allocates the item id.** A launch order names a request id
  (`OrdnanceLaunchId`) for once-only handling, but the `ProjectileId` comes
  from the session's own serial, so a replayed request can never name a second
  item and two producers cannot collide on an id.
- **The engagement is carried by the session, not the item.** The damage node
  and the stable status recipient come from the producer at launch and live in
  a session map keyed by `ProjectileId`, released only with the item. The item
  itself carries no target geometry, and the runtime never sees the ECS.
- **The step consumes its tick.** Movement, fuse decisions and expiry are
  once-per-tick, so a repeated tick is refused whole (`StaleTick`) rather than
  made idempotent. An advance refusal is not retryable — the tick was entered,
  and the report says which part failed.
- **The logs and the per-tick report hold the same records.** A consumer
  drains `effects()`/`events()` *or* reads `OrdnanceSessionTick::effects`/
  `events`, never both, or every launch sounds twice. This is the same
  double-report surface F27-C's `WeaponEffectLog` documents.
- **A guidance-loss detonation is reported but its blast is not applied.** The
  runtime removes the item and reports it in `GuidanceTick::detonated` with no
  position and no runtime method to route it, so this stage releases its
  engagement and reports the detonation; applying the blast is a new runtime
  decision (what a lost-target detonation damages, at what radius) and is
  filed as a follow-up rather than invented.

## Unknowns recorded (not guessed)

Everything numeric here is the F28-A/F28-B synthetic fixture's own value
carried unchanged, except the two retimed tests, which set the declared fuse
and the area's own bounded lifetime below the item lifetime so the timed fuse
can fire at all. None of it is original. The unmeasured original facts are the
ones F28-A and F28-B already list — the catalogue, the fuse shape and arming
condition, every lost-target behavior, the nitro activation/burn/recovery/
tradeoff facts, and whether an area effect damages, chokes, stalls or marks.
They are not duplicated here; F28-D and tasks #452/#454 own measuring them.
The F28-C-specific unknowns are:

- **The cockpit's launch and nitro controls, hardpoint firing order and ammo
  cycling** — `OrdnanceOrder` is the boundary; the original's controls are
  unmeasured (F46-B/F44-B).
- **The AI's decision to launch and which component it chooses** — the AI
  producer is a `OrdnanceOrder` issuer this stage defines but does not write
  (F32-B).
- **The network packet encoding of `OrdnanceNetworkEvent`** — the event ids are
  designed so a later encoder can be added without changing the gameplay path
  (F57-B).
- **What a lost-target `Detonate` blast applies** — filed as a follow-up.

## Test sensitivity (measured, one mutation at a time)

Four mutations were applied to `crates/cs_app/src/ordnance.rs`, one at a time,
and `cargo test -p cs_app --test accept_f28_c_ordnance_session_step` was re-run
after each; the file was restored from a byte-identical backup (`shasum`
`6fb111a2e3435c891ed2a122e103a7a1c2f8f5d9`) after every probe. Every mutation
was caught:

| mutation | caught by | result |
| --- | --- | --- |
| the status advance is neutralised (no expiry is ever reported) | `accept_f28_c_a_timed_engine_status_expires_on_its_tick_and_resets_on_restart` | 1 failed |
| the ECS mirror pass is removed | `accept_f28_c_a_launch_reads_the_live_pose_and_mirrors_the_item`, `accept_f28_c_a_mirror_from_another_generation_is_reconciled`, `accept_f28_c_teardown_releases_the_items_and_refuses_later_orders` | 3 failed |
| the live launcher walk is replaced with an empty read | 9 of 12 (everything but the standing-request, generation-zero and foreign-registration tests) | 9 failed |
| the authority's resolution is not recorded from the routed hits | `accept_f28_c_a_triggered_item_routes_its_damage_once_into_the_authority` | 1 failed |

The minimum scenario is the first row: the timed effect is applied on the tick
its item triggers, is live to its boundary and must be reported expired on the
exact boundary tick. Removing the expiry handling turns it red.

## Not claimed

No original-data verification, no claim that any ordnance family, fuse,
guidance rule, status effect or nitro parameter matches the 2000 original, no
audio or visual verification of the emitted effects, no Avian body, no cockpit
input device, no AI launch producer, no network packet encoding, no scheduling
of the step, no flight-model consumption of `EngineStatus`, and no guidance
blast application. This stage awards at most **checked**; F28-D and the owner's
evidence gate the rest.
