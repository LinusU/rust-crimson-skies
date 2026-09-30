# F29-A: Damage graphs, hit ordering and lifecycle events

Date: 2026-09-30. Task: F29-A "Define damage graphs, hit ordering and
lifecycle events" (`specs/F29-damage-zones-armor-destruction-and-bailout.md`,
section `### F29-A`). Shared contract:
`docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used: ordinary
build/test only (no `CS_GAME_DIR` read, no evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/damage/{mod,graph,events,resolver,synthetic}.rs`
  (new): the runtime contract — `DamageGraph`/`DamageNode`/`DamageNodeKey`/
  `DamageNodeKind`/`DamageChannel`/`SystemKind`/`PartState`, the immutable
  `HitEvent` input with `HitEventId`, the ordered `DamageEvent` output with
  `DamageEventId`, the five `LifecycleKind`s, the `AttributionRule`
  vocabulary, `DamagePolicy`, `DamageError`, the per-session
  `DamageResolver` (each actor registering its own graph's declared
  policy), and the `synthetic_airframe_graph` fixture.
- `crates/cs_content/src/damage.rs` (new): the declared,
  provenance-carrying schema — `DeclaredDamageGraph`/`DeclaredDamageNode`
  with `Origin`, per-node `Resolved` integrity pools and
  `SceneNodeId` part bindings, `GraphSubjectKind` (aircraft / world object
  / capital ship: same identity discipline, own rules), `GraphRules` with
  `Resolved<AttributionRule>`, and `declared_synthetic_airframe_damage`.
- `crates/cs_app/src/damage.rs` (new): `lower_graph` and `lower_policy`
  (the declared→runtime conversion boundary carrying `Resolved::Unknown`
  through for node values and refusing an unknown attribution outright)
  plus the generation-stamped `DamageActorBinding` ECS record.
- `crates/cs_sim/src/lib.rs`, `crates/cs_content/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): module declarations and docs.
- `crates/cs_sim/tests/accept_f29_a_damage_resolver.rs` (16 tests),
  `crates/cs_content/tests/accept_f29_a_damage_schema.rs` (6 tests),
  `crates/cs_app/tests/accept_f29_a_damage_boundary.rs` (5 tests): the
  `accept_f29_a_*` acceptance tests.
- This file.

**One observable failure:** without the once-per-actor terminal pass, two
same-tick lethal hits on the hull emit two destructions and two kill
awards — `accept_f29_a_two_same_tick_lethal_hits_award_a_single_kill`
counts 1 `Lifecycle{Destroyed}` and 1 `KillAwarded` credited to the earlier
hit in id order. Without the declared ordering rule, feeding the same hits
in reverse order changes the award — the same test feeds the batch reversed
and `accept_f29_a_resolution_is_deterministic_under_input_shuffle`
compares full event sequences. Every test calls production code (the
resolver, the graph validators, the lowering functions), so removing any
layer fails to compile.

## Semantics defined at this stage

- **Damage graph.** A `DamageGraph` is one subject's (`ContentId`)
  damage model: `DamageNode`s keyed by stable `DamageNodeKey`s (the
  content-id grammar applied to graph-local identity) with kinds
  `ArmorZone | InternalStructure | Engine | WeaponMount`, a `Resolved`
  integrity pool each, a `lethal` flag, an optional disabled `SystemKind`,
  and two edge kinds: `guarded_by` (the armor node that takes
  `DamageChannel::Armor` hits for this node first) and `overflow` (where a
  hit's remainder flows once the node is depleted). Validation rejects
  duplicate keys, dangling/self edges, a non-armor or armor-guarded guard,
  an overflow cycle, and a negative or non-finite *known* pool.
- **Hit ordering.** `DamageResolver::resolve(tick, hits)` validates that
  every hit carries the resolver's session and the resolved tick plus a
  unique `HitEventId`, then resolves in ascending id order — session,
  tick, producer, sequence. The event sequence is therefore a function of
  the hit set, not of producer append order.
- **Channel routing.** `Armor` enters through the target's `guarded_by`
  armor node when declared; `Internal` enters the named node directly.
  Each hop applies `min(remaining_damage, pool)` — armor and internal
  pools are distinct, so equal raw damage produces distinguishable results
  with no multiplier (AC02's model; the zone/disablement implementation is
  F29-B). A depleted node is transparent: the rest of the shot flows along
  `overflow`. A node whose pool is `Resolved::Unknown` emits `HitBlocked`
  with its claim — never guessed.
- **Simultaneous lethals.** The first hit in resolved order that depletes
  a `lethal` node is the recorded *blow*. After the batch, each newly
  destroyed actor emits one `Lifecycle{Destroyed}` and one `KillAwarded`
  computed under *that actor's own* declared `AttributionRule` — the
  policy registers with the actor
  (`DamageResolver::register_actor(actor, graph, policy)`), so an
  aircraft, a world object and a capital ship keep their own rules in one
  session. `FirstLethalHit` credits the blow's attacker;
  `GreatestDamage` credits the attacker whose hits applied the most
  damage to the victim this tick (tie → earliest contributing hit →
  lower actor id). An attackerless (world) blow credits `None`. An actor
  already `Destroyed` can take further part damage but emits no second
  destruction or award — on the same tick or any later one (AC01 +
  non-negotiable 2). A record closed by a terminal transition is the same
  kind of once-only boundary: hits still apply part damage, but no
  lifecycle or scoring event is recorded for the closed actor again.
- **Lifecycle separation.** `LifecycleKind` is five distinct transitions —
  `Destroyed`, `PilotBailout`, `OwnershipCaptured`, `Despawned`,
  `MissionRemoved` (non-negotiable 3). Damage emits only `Destroyed`; the
  others are recorded through `record_lifecycle` by the systems that own
  them, each kind at most once per actor. `Despawned` and `MissionRemoved`
  are terminal: the record closes and nothing may be recorded for the
  actor again.
- **Session confinement.** The resolver is session-scoped; hits,
  registrations and lifecycle records from another generation are refused
  (`ForeignSession`) — a restart or swap is a new resolver, never a
  mutated old one (non-negotiable 5; STATE-TRANSACTIONS generations).
- **Boundary.** `lower_graph` maps the declared record field-wise and
  carries every `Resolved::Unknown` through verbatim; `lower_policy`
  produces the `DamagePolicy` the actor registers under and refuses an
  unknown `lethal_attribution` with its claim, so no session resolves
  kills under an unstated rule.

## Designed vocabulary, not original data

Every record, kind, rule and fixture value here is **newly authored
project design** (`Origin::Designed`/`Origin::SyntheticFixture`,
`ClaimStatus::Designed` provenance). Unknown and not guessed:

- the original damage model: zone set, whether armor depleted as a pool or
  scaled damage, overkill propagation and kill attribution (the manual and
  guides name no damage equations — `docs/research/FINDINGS.md`;
  F29 "Research boundary");
- whether the original distinguishes armor-piercing/internal damage at
  all, and on which channel its hits ran;
- which original files declare which parts; the importer and its evidence
  are F29-B/D;
- the original bailout rule, its input gesture and its mission-result
  policy (non-negotiable 4: evidence-backed, not parachute-rendering —
  F29-C/D);
- whether the original applied damage to destroyed-actor wrecks in the
  same tick.

## Follow-ups that gate later stages

1. **`cs_types` still has no `SessionId`/`EventId`/`ActorId`/node-key
   types** although `IDENTITY-CONTENT` specifies them. `ActorId`,
   `HitEventId` and `DamageEventId` carry the contract's fields as
   damage-scoped types (the same pattern `AnimationEventId` used, filed
   by F20-A), and `DamageNodeKey` duplicates the content-key grammar in
   both `cs_sim` and `cs_content` because `cs_types` is outside this
   task's owner paths. Migrate to the shared types when they exist.
2. **No actor spawn/registration wiring.** The resolver is a plain
   session object; which producer emits `HitEvent`s (weapons F27/F28,
   contacts F23-B), how `DamageActorBinding`s spawn, and where
   `resolve`/`record_lifecycle` sit in the Bevy schedule are F29-B/C.
3. **Bailout/capture are vocabulary only.** Their producers (input
   confirmation, ownership transfer, mission-result policy) are F29-C/D;
   nothing here grants survival or mission credit for a bailout.
4. **System disablement is a record.** `SystemDisabled` says the mount's
   `Weapon` went down; gating actual firing and the `NodeDisabled` visual
   consumption (the `cs_app::scene::AirframeDamageState` pathway) is
   F29-B/C.
5. **No authored-initial-damage path.** `register_actor` starts pristine;
   applying a mission's authored initial damage (the `WorldInstance`
   `initially_damaged` records of F18-A) needs a registration variant —
   recorded for F29-B.
6. **`DamageState` duplication with `cs_sim::flight`.** F24-A's
   `flight::DamageState` scales flight authority; the resolver's part
   states are the producer side. F29-B decides how part destruction maps
   onto the flight authorities; no mapping is assumed here.

## Review notes (2026-09-30)

Reviewer: Jakob - Devin SWE-2/devin-1 — the same agent name that
implemented the task, but a **fresh session and context** (the review
claim began with `git diff origin/main...HEAD`, not the implementation
transcript). A same-name review is not independent original-reference
evidence; it is recorded here per the review policy.

Fixes made during review:

1. **Per-actor `DamagePolicy`.** The implementer's resolver held one
   session-wide policy, so two actors whose graphs declare different
   attribution rules could not coexist — contradicting the deliverable's
   "the same identity discipline but their own rules". `register_actor`
   now takes the actor's `DamagePolicy` (`register_actor(actor, graph,
   policy)`) and the terminal pass resolves each victim under its own
   rule. `DamageResolver::new` lost its policy argument and
   `DamageResolver::policy(actor)` reports a registration's rules.
   Covered by the new
   `accept_f29_a_actors_resolve_under_their_own_declared_rules`.
2. **Closed records record nothing again.** `resolve`'s terminal pass
   never checked `state.terminal`, so a `Despawned`/`MissionRemoved`
   actor taking a lethal hit still emitted `Lifecycle{Destroyed}` and a
   `KillAwarded` — defeating the closed-record guarantee
   `record_lifecycle` enforces. Part damage still applies; lifecycle and
   scoring are suppressed. Covered by the new
   `accept_f29_a_terminal_record_emits_no_destruction_or_award`.
3. **`part_state` honours its `None` contract.** The doc promised `None`
   "when the actor or node is unknown" but a node outside the graph
   returned `Some(PartState::Unknown)`, conflating "no such node" with
   "unresolved pool". It now returns `None` for an unknown node or actor
   and `Some(PartState::Unknown)` only for a real unresolved pool;
   asserted inside
   `accept_f29_a_unknown_targets_and_nodes_are_refused_visibly`.

Sensitivity probes run by the reviewer (each reverted afterwards; no
probe committed):

1. Removing the new terminal check →
   `accept_f29_a_terminal_record_emits_no_destruction_or_award` failed
   with `Lifecycle{Destroyed}` emitted for the despawned actor.
2. Disabling the `sort_by_key` hit ordering →
   `accept_f29_a_two_same_tick_lethal_hits_award_a_single_kill` and
   `accept_f29_a_resolution_is_deterministic_under_input_shuffle` both
   failed (the reversed-input batch credited the wrong attacker).

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-A`),
  `docs/contracts/STATE-TRANSACTIONS.md`, `docs/contracts/IDENTITY-CONTENT.md`.
- `docs/research/FINDINGS.md` (no damage equations recovered), S13.
- `docs/01-ARCHITECTURE.md` (simulation schedule's resolution phase,
  stable identity).
