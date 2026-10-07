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
  has ended (`EliminateEnemies`, `LastSideStanding` with the declared tie
  outcome, `SurviveToDeadline`, honouring the respawn budget). `ScenarioOutcome`
  carries subject, seed, result and tick only — no cash, node, unlock, run or
  profile field — so settling an IA outcome cannot name campaign state.

## Deviations and open items

* `crates/cs_sim/src/scenario.rs` was **not** created. `cs_sim` depends only on
  `cs_script` and `cs_types`, so it cannot see `VictoryCondition` or the lowered
  scenario; evaluation therefore lives beside the lowering in `cs_app`. Moving
  it needs the declared types in a crate `cs_sim` may depend on.
* Writing an outcome into the IA record scopes (and AC03: complete/retry IA with
  campaign cash and progression unchanged) is F49-C's wiring. The campaign
  `OutcomeId` is deliberately not reused: it needs a profile, run and session.
* Spawning the lowered actors into a live mission runtime is not part of this
  slice; the snapshot is supplied by the caller.
* The end-condition semantics, tie handling and respawn accounting are designed,
  not measured: a respawn budget is counted as a single `respawns_used` figure
  compared with `per_side`, not per side.
