# F30-B: Selection actions and threat state

Date: 2026-10-02. Task: F30-B "Implement original selection actions and
threat state" (`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
section `### F30-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/targeting.rs`: `SelectionAction`, `SelectionBinding`,
  `SelectionFrame`, `TargetFilter::label`, `CycleDirection::label`,
  `AttackEvent::from_hit`, `ThreatFeed`, `TargetStore::record_hits`,
  `TargetStore::registered`/`unregister`, `TargetStore::act`,
  `TargetStore::crosshair_query`, `TargetStore::set_class`/`set_objective`,
  `Reticle`, `TargetPhase`, `TargetStore::phase`,
  `TargetError::MissingCrosshair` and the `synthetic_selection_binding`
  fixture.
- `crates/cs_content/src/target_rules.rs`: the F30-B declared action table —
  `DeclaredAction`, `DeclaredSelectionAction`, `DeclaredSelectionActions`,
  `SelectionActionsError` validation and
  `declared_synthetic_selection_actions`.
- `crates/cs_app/src/targeting.rs`: `lower_selection_actions`,
  `TargetLowerError::UnknownAction`, `TargetableState`, `TargetingSession`,
  `TargetingError`, `RosterReport`, `sync_targetable_roster`,
  `SelectionReport`, `apply_selection_edges`, `TargetDamageTick`,
  `TargetDamageReport`, `apply_target_damage`.
- `crates/cs_sim/src/lib.rs`, `crates/cs_content/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): module documentation.
- `crates/cs_sim/tests/accept_f30_b_selection_actions.rs` (7 tests),
  `crates/cs_content/tests/accept_f30_b_target_actions.rs` (3 tests),
  `crates/cs_app/tests/accept_f30_b_targeting_session.rs` (8 tests).
- This file.

**One observable failure:** without the single phase record, the reticle and
the AI hostility gate are two reads and a faction change can land between
them. `accept_f30_b_faction_change_updates_reticle_and_ai_hostility_in_one_phase`
selects a raider through the bound `TargetNext` edge, reads
`TargetStore::phase` once (allegiance `Hostile`, `hostile: true`) and hands
that reticle to the **real** `cs_sim::ai::combat` planner, which accepts the
candidate; after `set_faction` into the player's faction the next phase
reports `Friendly`/`hostile: false` and the same planner refuses the same
candidate with `RejectReason::NotHostile`. A reticle that cached its
allegiance, or an AI gate reading the store directly, fails the second half.

## Semantics defined at this stage

- **Actions.** `SelectionAction` is the bound action (`Cycle`/`Nearest`/
  `NearestAttacker`/`UnderCrosshair`/`Clear`); `SelectionRequest` stays the
  evaluated request. The difference is who supplies the phase context: an
  action names *what* to select, and `TargetStore::act` takes the tick and
  the crosshair ray from the session's `SelectionFrame`. A command edge is
  therefore plain data.
- **Bindings.** `SelectionBinding` is a `BTreeMap<FlightCommand,
  SelectionAction>`, so iterating it is a total order over commands. An
  unbound command is not a targeting command and is reported as such; it is
  never guessed into an action.
- **Under-crosshair.** An action needing a ray refuses with
  `TargetError::MissingCrosshair` when the frame carries none — a frame
  without a ray is missing evidence, not evidence of an empty ray.
  `TargetStore::crosshair_query` substitutes the declared
  `TargetPolicy::crosshair_cone` when the producer supplies no cone and
  validates any cone at the boundary.
- **Threat feed.** `AttackEvent::from_hit` turns the damage system's own
  `HitEvent` into a ledger entry: evidence is the hit's own `HitEventId`,
  `at` is that id's tick. A hit with no attributable source and a self-hit
  credit nobody and mint nothing. `TargetStore::record_hits` checks the
  whole batch's generation first (a foreign hit refuses the batch rather
  than half-recording it), is idempotent per evidence id, and *counts*
  rather than refuses hits naming untracked actors — damage records exist
  for parts targeting never listed.
- **Roster retirement.** `TargetStore::unregister` is the *entity left the
  world* transaction: it drops the actor from the roster and from every
  ledger in both directions. It is deliberately **not** the same statement
  as a recorded `LifecycleKind::Destroyed`, which keeps its attack evidence
  (F30-A's acceptance case asserts exactly that). A departed actor cannot be
  a live threat cue or a selectable target.
- **Phase record.** `TargetStore::phase` prunes the selection first, then
  derives `Reticle` (class, faction, live allegiance, `hostile`, `threatening`,
  objective, revealed, canonical position, distance) and the observer's
  threat cues from the same reads. `hostile` is true only for a *declared*
  hostile relation, matching the AI's own gate; a neutral, friendly or
  undeclared pair is not hostile.
- **ECS session.** `TargetingSession` owns the store, the selection, the
  lowered binding and the last `TargetPhase`. `sync_targetable_roster`
  registers/updates from `(TargetableBinding, TargetableState)`, unregisters
  actors whose entity is gone, ignores another scene generation or another
  session generation, and reports a binding with no record as `incomplete`
  rather than registering a guessed position. Every mutable record field has
  a store transaction to travel through — including F30-B's
  `set_class`/`set_objective`, added because a reclassification written on
  an entity would otherwise be *reported* as an update and silently *not
  applied*, which would make the roster sync's own report a lie. A class or
  objective change is a scripted statement, not a pose, so it reaches the
  next query the same way a capture does. `apply_selection_edges` runs each
  bound edge in arrival order and derives the phase once at the end.
  `apply_target_damage` records every `Lifecycle` event and mints attacks
  only for hits the resolver reported `HitApplied`.
- **Declared actions.** `DeclaredSelectionActions` maps a typed command edge
  to a `Resolved<DeclaredAction>` with its own provenance. `try_new` refuses
  a continuous axis (a target action would fire every frame the axis moved)
  and a duplicated command (one edge runs one action);
  `lower_selection_actions` refuses `Resolved::Unknown` with
  `TargetLowerError::UnknownAction` — an unevidenced action is not the same
  statement as "this command is not a target command".

## Unknowns recorded (not guessed)

- Which selection actions the original 2000 PC game binds, to which keys,
  whether a cycle ever walks allies or objectives, the original cycle order,
  crosshair cone, threat window, reveal rules and assistance behavior are
  all unmeasured — F30-D's retail stage. Nothing here claims
  `verified_original`; every fixture value is designed and carries
  `Origin::SyntheticFixture` with designed provenance.
- Whether the original threat cue is per-attacker, per-direction or
  per-weapon is unmeasured; the ledger records one cue per attacker, which
  is the designed rule.
- Whether an *undeclared* faction pair may be selected by a "next target"
  edge at all is unmeasured. The designed rule is the F30-A one: an
  undeclared relation matches no allegiance filter, so a bound hostile cycle
  skips it rather than treating it as hostile.
- The synthetic fixture binds the nearest-attacker action to the
  weapon-cycle edge purely so the declared table exercises a non-target key;
  that binding is designed fixture content, not a claim about the original
  game's controls.
- Whether the original game lets a bailed-out or captured actor stay
  selectable, and whether occlusion uses the geometry the contract hands it,
  remain designed contract decisions (F30-A findings).

## Not claimed

No original-data verification, no HUD/spyglass/weapon consumer, no schedule
wiring and no reticle rendering — F30-C/D own those. The task awards at most
**checked** status.

## Reviewer sensitivity probes

The implementer ran these probes before handing over (each reverted
afterwards; no probe is committed, and the branch is byte-identical to the
pushed commit afterwards):

| Probe | Result |
| --- | --- |
| 1. `Reticle::hostile` hardcoded to `true` (a reticle that does not re-derive hostility) | `accept_f30_b_faction_change_updates_reticle_and_ai_hostility_in_one_phase` and `accept_f30_b_declared_filters_reach_the_bound_actions` **failed** |
| 2. `apply_target_damage` mints from every submitted hit instead of the resolver's `HitApplied` | `accept_f30_b_damage_tick_feeds_threats_and_lifecycle` **failed** (`applied` 2 vs 1) |
| 3. a frame with no crosshair ray answers "no target" instead of `MissingCrosshair` | `accept_f30_b_command_edges_run_the_bound_actions` and `accept_f30_b_session_runs_edges_and_publishes_the_phase_record` **failed** |
| 4. the threat ledger purged on `record_lifecycle` instead of on `unregister` | `accept_f30_b_destroyed_selection_clears_in_the_phase_record` **failed** on the surviving-evidence assertion |
| 5. repeated edges coalesced within one frame | `accept_f30_b_session_runs_edges_and_publishes_the_phase_record` **failed** |
| 6. the whole F30-B production path removed from `cs_sim::targeting` | the three acceptance files fail to compile — the tests call production code, not a parallel test-only implementation |

A defect the implementer found by reviewing the diff rather than by a
probe: the roster sync compared the entity's class and objective flag
against the store's record and counted an "updated" pass, but F30-A's store
had no transaction to apply either one — so a scripted reclassification
would have been reported as applied and silently dropped. `set_class` and
`set_objective` were added, the sync writes through every field's
transaction, and `accept_f30_b_reclassification_on_an_entity_reaches_the_actions`
discriminates it (drop either transaction and the ordered queries keep the
old answers).