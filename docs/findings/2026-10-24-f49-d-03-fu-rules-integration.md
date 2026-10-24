# F49-D.03-FU-RULES (#1583): integration of the three measurements

Date: 2026-10-24. Parent of #1638 (END), #1639 (ROSTER), #1640 (DIFF), all merged.
Provenance of every statement: `static` (decrypted engine image + retail `ia.zrd`),
never `verified_original`. Nothing was run, rendered or played.

| question in #1583 | answer | where |
| --- | --- | --- |
| how each mission type ends | measured and bound (`measured_ia_victory_rules`); no deadline, no respawn budget | `2026-10-19-f49-d-03-fu-rules-end.md` |
| which aircraft each type spawns, the wingman planes | measured and bound (ace for `dogfight_ace` only; wingmen are row 5) | `2026-10-24-f49-d-03-fu-rules-roster.md` |
| `num_wingmen` / `player_plane` of `ZBD/c2b/IA1` | 0 / row 5 (engine defaults), resolved | roster finding, "`ZBD/c2b/IA1` re-verified" |
| the per-scenario difficulty | **none exists**: `GameIntensity` is overwritten per spawn (ace from `ace_skill`, groups always 1); `enemy_skill` is not read | `2026-10-24-f49-d-03-fu-rules-difficulty.md` |

## Consequence for the catalog

Rules and rosters are measured for every retail preset. `difficulty` stays
`Declared::Unmeasured` (claim `f49.d.03.unmeasured.difficulty`) on purpose: the
original has no scenario-level value to put in `ScenarioParameters::difficulty`,
and inventing a `DifficultyTier` would be a design, not a measurement. So
`lower_preset` still refuses retail presets, now on difficulty (then factions,
loadouts), which is the honest state. Lowering the ace/group intensity split needs
per-actor intensity in the scenario schema (follow-up task filed).

## Still open (not resolved here)

* What the player and wingmen are created under (no `GameIntensity` write traced).
* Whether the menu's mission type survives the load (roster finding, "Unmeasured").
* Factions, loadouts and survivability: #1582 and the existing follow-ups.
