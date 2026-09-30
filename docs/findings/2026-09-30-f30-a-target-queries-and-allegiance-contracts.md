# F30-A: Target queries and allegiance contracts

Date: 2026-09-30. Task: F30-A "Define target queries and allegiance
contracts" (`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
section `### F30-A`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only (no `CS_GAME_DIR` read, no evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/targeting.rs` (new): the runtime contract —
  `TargetStore` (per-session roster, allegiance table, threat ledger),
  `TargetRecord`, `TargetClass`, `Allegiance`, `AllegianceTable`,
  `TargetPolicy`, `TargetFilter`, `CycleDirection`, `SelectionRequest`,
  `CrosshairQuery`/`CrosshairError`, `TargetSelection`, `TargetInfo`,
  `AttackEvent`/`ThreatCue`, `TargetError`, plus the synthetic fixture
  (`synthetic_allegiance_table`, `synthetic_target_policy`,
  `synthetic_roster`, faction id helpers).
- `crates/cs_content/src/target_rules.rs` (new): the declared,
  provenance-carrying schema — `DeclaredTargetRules` (subject, `Origin`,
  faction set, directed `DeclaredRelation`s, `TargetRuleSet`,
  `Provenance`), `DeclaredAllegiance`, `TargetRulesError` validation, and
  `declared_synthetic_target_rules`.
- `crates/cs_app/src/targeting.rs` (new): `lower_rules` (declared →
  runtime conversion; every `Resolved::Unknown` refuses rather than
  guessing) and the generation-stamped `TargetableBinding` ECS record.
- `crates/cs_sim/src/lib.rs`, `crates/cs_content/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): module declarations and docs.
- `crates/cs_sim/tests/accept_f30_a_targeting.rs` (7 tests),
  `crates/cs_content/tests/accept_f30_a_target_rules.rs` (3 tests),
  `crates/cs_app/tests/accept_f30_a_targeting_boundary.rs` (4 tests): the
  `accept_f30_a_*` acceptance tests.
- This file.

**One observable failure:** without the `(distance², ActorId)` total
order, cycling the three raiders parked exactly 100 m from the observer
depends on roster insertion order — `accept_f30_a_equal_distance_targets_cycle_deterministically`
registers the roster in two different orders and asserts the same cycle
sequence `2, 5, 9, 2, 5` (serials deliberately out of coordinate order).
Every test calls production code (the store, the schema validator, the
lowering function), so removing a layer fails to compile.

## Semantics defined at this stage

- **Roster.** `TargetStore::new(session, policy, allegiance)` is the
  per-session authority (STATE-TRANSACTIONS session generations; a restart
  or aircraft swap is a new store). `TargetRecord` carries `ActorId`
  (session-qualified, reusing `cs_sim::damage::ActorId` — `cs_types` owns
  no shared `ActorId` yet, already recorded in the F29-A findings), the
  faction `ContentId`, `TargetClass`, objective flag, reveal state,
  script-phase eligibility and the canonical f64 `WorldPosition`.
- **Eligibility.** `eligible` = registered, no ending lifecycle
  transition recorded, revealed, phase-eligible. Destruction, despawn and
  mission removal end targetability; bailout and capture deliberately do
  not (designed split — a bailed-out airframe is still a physical object;
  capture is an ownership change that `set_faction` records).
- **Allegiance.** Directed `(from, to)` declared pairs; `from == to` is
  the contract's own `Friendly` invariant. An undeclared pair is `None` —
  never a guessed hostility — and matches no allegiance filter.
  `set_faction` (ownership change) and `set_allegiance` (scripted
  relation change) reclassify on the next query, in the same phase
  boundary (AC02's contract half).
- **Ordering.** `ordered(observer, filter)` sorts eligible actors by
  ascending squared canonical distance, ties on `ActorId` — a total order
  independent of insertion/iteration order (non-negotiable 2, AC01).
- **Selection.** `apply(observer, selection, request)` evaluates
  `Cycle { direction, filter }`, `Nearest { filter }`,
  `NearestAttacker { now }`, `UnderCrosshair(query)` and `Clear`
  deterministically. `prune` clears a held selection whose target stopped
  being eligible before any consumer renders it (AC03's contract half).
- **Crosshair.** `CrosshairQuery` carries the ray, the validated cone
  half-angle `[0, π]` and the producer-supplied `occluded` set; selection
  picks the eligible unoccluded actor nearest the ray, distance then
  actor id breaking angular ties.
- **Threats.** `record_attack` accepts only session-qualified
  `AttackEvent`s evidenced by the damage system's `HitEventId` between
  registered actors; `threats(victim, now)` reports one `ThreatCue` per
  attacker inside the declared `threat_window_ticks` — cues come from
  authoritative attack events, never proximity (non-negotiable 4).
- **Declared schema.** `DeclaredTargetRules` validates the faction
  namespace, unique factions, declared endpoints, no self-relations, no
  duplicated directed pairs and a finite in-range known cone. The
  lead-indicator and aim-assistance options are separate
  `Resolved<bool>`s with their own provenance (non-negotiable 3).
- **Lowering.** `cs_app::targeting::lower_rules` maps field-wise and
  *refuses* any `Resolved::Unknown` (rule value or relation) — "unknown"
  is not the same statement as "no relation", and no session runs under
  a guessed window, cone or assistance flag.

## Unknowns recorded (not guessed)

- The original 2000 PC game's target-cycle order, which selection
  actions it binds (enemy/objective/ally/non-aircraft/nearest-attacker/
  under-crosshair/clear are spec-named; the `SelectionRequest` vocabulary
  is designed), its crosshair cone, reveal rules, threat window and
  assistance behavior are all unmeasured — F30-D's retail stage. Nothing
  here claims `verified_original`; every fixture value is designed.
- Whether a bailed-out or captured actor stays targetable, and whether
  occlusion uses the geometry the contract hands it, are designed
  contract decisions pending original evidence.
- `SelectionRequest` is not yet bound to `FlightCommand::TargetNext`/
  `TargetPrev` or the declared action set — that mapping is F30-B.

## Not claimed

No original-data verification, no runtime wiring into the app schedule,
no HUD/spyglass/weapon consumer — F30-B/C/D own those. The task awards at
most **checked** status.
