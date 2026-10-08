# F49-B: scenario normalization and isolated outcomes

Task F49-B (`accept_f49_b_`). Everything here is **designed engine behaviour on
synthetic fixtures**; the original Instant Action catalog, end conditions and
record rules remain unmeasured (see the F49-A finding) and F49-D owns them.

## What was built

* `CustomScenarioDraft::roster` / `replace_roster_slot`
  (`crates/cs_content/src/instant_action.rs`): replaces the one actor holding a
  `(side, slot)`; it never adds or reorders, and an absent slot is the new
  `ScenarioSchemaError::NoSuchRosterSlot`.
* `ui::instant_action::diff_scenarios` (`normalize.rs`): matches actors by
  `(side, slot)` and reports per-actor field changes and scenario-level changes.
  This is how AC02 is observed on the production `lower_custom` path.
* `ui::instant_action::evaluate_outcome`: decides whether a lowered scenario
  has ended (`EliminateEnemies` and `LastSideStanding` ending on which
  coalition is still flying, `SurviveToDeadline` ending on its deadline).
  Replacement budgets are counted **per `ScenarioSide`**, as
  `RespawnBudget::PerSide` declares, so what one coalition spent never decides
  whether the other can still replace; a mutual wipe on one tick is the tie the
  declared `TieOutcome` resolves under both wipe conditions. `ScenarioOutcome`
  carries subject, seed, result and tick only — no cash, node, unlock, run or
  profile field — so settling an IA outcome cannot name campaign state.

## Deviations and open items

* `crates/cs_sim/src/scenario.rs` was **not** created. `cs_sim` may depend only
  on `cs_types` and `cs_script` (`docs/01-ARCHITECTURE.md`), so it can see
  neither `cs_content::instant_action::VictoryCondition` nor the `LoweredScenario`
  this module evaluates; evaluation therefore lives beside the lowering in
  `cs_app`. Moving it needs a dependency edge the architecture does not grant.
* Writing an outcome into the IA record scopes (and AC03: complete/retry IA with
  campaign cash and progression unchanged) is F49-C's wiring. The campaign
  `OutcomeId` is deliberately not reused: it needs a profile, run and session.
* Spawning the lowered actors into a live mission runtime is not part of this
  slice; the snapshot is supplied by the caller.
* The end-condition semantics, tie handling and respawn accounting are designed,
  not measured: how the original decides an Instant Action has ended, and what
  it records, is still unmeasured and belongs to F49-D's retail stage. A
  coalition is "not yet out" while any side of it **that flies in the
  scenario** has budget left, and neutral actors are counted by neither
  coalition.

## Review corrections (2026-10-08, reviewer bunny-alpha-1)

* The first cut counted `respawns_used` as **one** figure compared with
  `per_side`, so replacements the enemy side had spent also counted as the
  player's side having none, and the other way round: a side could be declared
  out while its own budget was untouched. `ScenarioSnapshot` now records
  spending per `ScenarioSide`, and
  `accept_f49_b_a_side_with_replacements_left_is_not_yet_out` pins each
  coalition's spending separately.
* The first cut resolved a mutual wipe under `EliminateEnemies` to `Defeat` by
  checking the player's side first, discarding the declared `TieOutcome` that
  `LastSideStanding` honours. Both wipe conditions now resolve a both-out tick
  through the declared tie outcome
  (`accept_f49_b_eliminate_enemies_resolves_a_mutual_wipe_with_the_declared_tie`).
* `accept_f49_b_neutral_traffic_never_decides_an_outcome` pins the neutral
  actors' side of the same rule, which was documented but untested.
