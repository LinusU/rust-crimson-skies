# PARITY2-M01-DAMAGE-EFFECTS (#1737): the measured level rule in the mission

Date: 2026-10-23. Implementer: `sonnet-1` (Sonnet 5.5), no independent review. Builds on
`2026-10-23-parity2-dmg-fx-levels.md` (#1979). Nothing here is `verified_original`; the original ran for no part of it.

## What the mission now does

* `dense_firetrail` (`player_smoketrail`) runs while the vehicle health fraction (sum of the four regions'
  remaining integrity over the sum of their pools) is at or below 0.10, not while a wing region is damaged
  (`damage_trail.rs`, `vehicle_health_fraction`). The pool is still the designed `DESIGNED_REGION_INTEGRITY`.
* Puff `COLORS` keys and `TEXTURE_SEQUENCE` frame times are fractions of the puff's life (trail and wing fire);
  no `COLORS` gives white at opacity `1 - fraction`; `GROWTH_FACTOR` scales the size 1x to Gx over the life.
* `prop1b` is drawn beside `prop1` (`PropellerBlurSecond`), shown by the same engine flag, translucent.
* The owner-command smoke hits the airframe twice: the gauge hits (vehicle fraction 0.55, no trail) and a critical
  hit (0.0375). Frames `damage_trail` (before) and `damage_trail_critical` (after), check `damage_trail_drawn`
  requires zero puffs before the second hit.

## Smoke result (release build, `--smoke-seconds 120`)

69 checks, no failures. `damage_trail_drawn`: 0 puffs before the second hit, 172 smoke and 691 fire puffs after.
Frames are under `private/evidence/1737/` (not committed).

## Still unmeasured or not done (rule 4)

* `FADE_RANGE` (depth unit of M01's camera not read), `PRIORITY`, `RANDOM_WEIGHT`, `PLAYER_1ST_PERSON` evaluation.
* `player_fuelleak` (0.85, `fuel_trail` at `pdp1..3`, a `RANDOM_WEIGHT` pick) is not drawn.
* Prop disc blend and default opacity; no ring rule exists in code or data, so no rings are drawn.
* The grey grade over the whole frame in the damage phase (filed by #1808) is unresolved.
* The original's real part pools; the 0.10 threshold is applied to the designed pools.
* `human_review` of the damaged frame beside the c10/c20 originals is still owed by the owner.
