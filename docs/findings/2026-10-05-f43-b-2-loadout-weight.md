# F43-B.2: loadout weight in the purchase rule

Task #622. Closes unknown #5 of
`2026-10-04-f43-b-progression-replay-and-purchase-transactions.md` (the
purchase rule's "weight" was not validated).

## Existing weight concept (reused, not redefined)

`cs_content::construction` (F44-A) already models weight:
`WeightUnits` (integer `Unit::GameWeight`), `ComponentQuote::mass`,
`PriceBook`, `ConstructionRules::max_mass` and `ConstructionRules::assess`,
which totals a blueprint with checked arithmetic and refuses unknowns. No second
weight type was added.

## Design

`cs_sim` may depend only on `cs_types` and `cs_script`, so it cannot name those
types. The purchase draft therefore carries the boundary's resolved verdict
input as integers:

* `cs_sim::campaign::LoadoutWeight::{Measured { total, limit }, Unknown { reason }}`
  is a required field of `PurchaseDraft`. There is no "unchecked" or "no limit"
  variant.
* `CampaignState::purchase` refuses `LoadoutOverweight` (total > limit; equal is
  inside) and `LoadoutWeightUnknown`. Order: availability, ownership, money,
  weight, expected revision last. Every refusal leaves the profile
  bit-identical.
* `cs_app::campaign::loadout_weight(rules, book, loadout)` resolves the verdict
  from `ConstructionRules::assess`. An unmeasured ceiling or mass, an unpriced
  component, an airframe mismatch or an overflowing total becomes `Unknown`
  carrying the named budget refusal.

## Limits of this stage (unknowns, not guesses)

1. **The campaign transaction trusts the boundary's numbers.** `cs_sim` judges
   the integers it is handed; it cannot verify that `total` was computed from the
   right loadout. Callers must use `loadout_weight`.
2. **`CampaignState` still has no loadout.** The *resulting* loadout (the
   blueprint after the purchase) is supplied by the caller; the campaign does
   not track which blueprint the player currently flies. That belongs to the
   F44 construction/purchase draft work.
3. **All masses and ceilings are synthetic.** The original component masses,
   the weight-unit scale and per-airframe ceilings are unmeasured (F44-D). The
   tests prove the rule with fixtures only; nothing here is a retail
   compatibility claim.
4. Whether the original refuses over-weight purchases at buy time or only
   when launching is unknown; the contract's wording ("before writing") is
   followed.
