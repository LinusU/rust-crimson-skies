# F44-A: Blueprint constraints and exact budget arithmetic

Date: 2026-10-01. Task: F44-A "Define blueprint constraints and exact budget
arithmetic" (`specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
section `### F44-A`). Shared contract:
`docs/contracts/STATE-TRANSACTIONS.md`, section "Outcome and economy
transaction". Capabilities used: ordinary build/test only (no `CS_GAME_DIR`
read, no evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/construction.rs` (new): `WeightUnits`, `MoneyMinor`,
  `DisplayMapping`, `ComponentQuote`, `PriceBook`, `ConstructionRules`,
  `ArmorFitment`, `GunFitment`, `OrdnanceFitment`, `DecalPlacement`,
  `PaintSelection`, `AircraftBlueprint`, `ConstructionSlot`,
  `ConstructionSchemaError`, `BudgetCategory`, `BudgetLine`,
  `BudgetBreakdown`, `BudgetQuantity`, `BlueprintTotals`, `LimitBreach`,
  `BudgetRefusal`, `BlueprintAssessment`, `ConstructionRules::assess`, the
  `SYNTHETIC_*` keys and fixture constructors.
- `crates/cs_content/src/lib.rs` (wiring only): `pub mod construction;` and
  one module-doc paragraph. No logic.
- `crates/cs_content/tests/accept_f44_a_budget_bounds.rs` (new): nine
  `accept_f44_a_*` tests.
- This file.

**Not created at this stage**, though they are the sheet's owner paths:
`crates/cs_sim/src/economy.rs` (F44-B's transactional purchase/sell draft and
availability check) and `crates/cs_app/src/construction/` (F44-C's editor, paint
and preview UI). Defining a currency transaction or a UI state machine here
would mean inventing an interaction model before the validator that both
consume exists — the same reason F09-A and F07-A recorded. The sheet's stage A
instruction is explicit: "Define typed inputs/outputs and a minimal synthetic
fixture first; do not jump ahead to a whole runtime."

**One observable failure:** a validator that uses `>=` instead of `>` when
comparing a total against its limit, or that rounds a total for display before
comparing it, accepts a loadout one unit over budget and a loadout one unit
over the price cap. `accept_f44_a_boundary_loadout_exactly_at_the_limits_is_accepted`
and its two one-unit-over siblings pin both sides: 6040 weight units against a
6040 ceiling is **inside**, and 6041 against the same ceiling is a
`LimitBreach::Mass` with `excess() == 1`, with the cost total untouched at
exactly 42000 against its own 42000 ceiling. Every mutation listed below breaks
at least one of them.

## What this stage decides, and how

- **Money and weight are integers, and the module contains no float.**
  `WeightUnits` is a `u64` count of `cs_types::content::Unit::GameWeight` and
  `MoneyMinor` a `u64` count of minor currency units, matching
  `IDENTITY-CONTENT` ("Integers represent money, ticks, counts, ammo and ids")
  and `STATE-TRANSACTIONS` ("Currency uses integer minor game units with a
  documented display mapping"). Kilograms were **not** chosen as the weight
  unit: the original's weight unit is unmeasured and `IDENTITY-CONTENT`
  forbids assuming SI for it, so the explicit `GameWeight` unit is the honest
  declaration and the conversion belongs to the reader that measures it.
- **Rounding is at a declared boundary, and that boundary is lossless.**
  `DisplayMapping::try_new(minor_per_major)` is the specification
  non-negotiable 2 asks for, and `MoneyMinor::format_display` renders with
  integer division and a zero-padded remainder at the divisor's full digit
  count. That turns out to be **injective** — `999999` and `1000000` do *not*
  collide at a divisor of `1_000_000` — which is a stronger property than "rounding
  is confined to display" and is asserted as such: the test round-trips seven
  totals back to their exact integers. `BlueprintAssessment::is_within_limits`
  reads only the integers and never consults a mapping, so no display step
  exists for an eligibility decision to be confused by.
- **The manual's rack numbers are profile data, never constants.**
  Non-negotiable 1 reports four gun positions and up to eight rocket
  hardpoints as *observed manual* constraints and requires confirming them
  "against each discovered airframe/rule profile before declaring universal
  limits". They are therefore `Resolved<u32>` fields of one airframe's
  `ConstructionRules`. `synthetic_wide_rack_rules()` declares six positions and
  two hardpoints **for the same airframe**, and
  `accept_f44_a_manual_rack_limits_are_profile_data_not_constants` shows one
  blueprint being accepted by one profile and refused by the other; if the
  limits were compiled-in constants, at least one verdict would be wrong.
- **A paired selection is accounted, not legalized.** `GunFitment::positions`
  is a `Resolved<u32>` carrying how many gun positions that selection consumes,
  so a mated pair occupies two and a single occupies one. Whether two guns may
  legally be mated on a given airframe, and which pairs are compatible, is
  F44-B's paired-gun rule — this stage only counts. An unmeasured count refuses
  the total instead of defaulting to one position, and a known count of zero is
  refused at construction so no fitment can free a slot.
- **Unknown is refused, never assumed.** Each of these produces a named
  `BudgetRefusal` and no totals at all: an unmeasured limit
  (`UnknownLimit`), a component the book does not quote (`NotPriced`), an
  unmeasured mass (`UnknownMass`) or price (`UnknownCost`), an unmeasured
  gun-position count (`UnknownGunPositions`), a blueprint on a different airframe
  (`AirframeMismatch`) and an `u64` overflow (`Overflow`). Refusals are ordered
  and deterministic — limits are resolved before any pricing work, because an
  unmeasurable ceiling means nothing can be concluded.
- **Overflow is refused rather than wrapped.** A wrapped `u64` total is
  *smaller* than the real one and would pass a limit check it should fail, so
  every sum is `checked_add`.
- **The budget is a closed, auditable set of categories.**
  `BudgetCategory::ALL` is exactly six rows and
  `BlueprintTotals::from_breakdown` folds over precisely that list, so a
  seventh category cannot appear without changing the arithmetic the totals are
  defined by. `BudgetBreakdown` exposes the per-category subtotals so a reviewer
  can add the columns and reach the same integers the totals report.
- **Paint is references only.** Non-negotiable 5 requires that an export/import
  never include copyrighted source textures implicitly, so `PaintSelection`
  holds `paint_mask` catalog ids and no pixel data, blob or path at all.
- **Structural refusals happen at construction.** Wrong-namespace ids, a
  repeated armor zone / weapon mount / hardpoint, a repeated equipment or paint
  entry, a duplicate price quote and a zero-position gun are all refused by
  `ConstructionSchemaError` before any arithmetic runs. The repeated-node error
  names *which* slot collided, since repairing an imported blueprint needs to
  know which one.

## Test inventory (`accept_f44_a_*`)

All nine are in `crates/cs_content/tests/accept_f44_a_budget_bounds.rs` and
call only the public `cs_content::construction` API.

| Test | Covers |
| --- | --- |
| `boundary_loadout_exactly_at_the_limits_is_accepted` | AC01 at-limit half: all four limits hit at once; totals equal the limits; the breakdown's columns re-sum to the totals |
| `one_weight_unit_over_the_limit_is_rejected` | AC01 weight half: 6041 vs 6040 gives `LimitBreach::Mass` with `excess() == 1`, cost untouched and not misattributed |
| `one_cost_unit_over_the_limit_is_rejected` | AC01 cost half: 42001 vs 42000 gives `LimitBreach::Cost` with `excess() == 1`, weight still exact — the two limits are independent |
| `display_rounding_cannot_change_purchase_eligibility` | non-negotiable 2: display mapping is injective and round-trips; the verdict is display-independent; a zero divisor is refused |
| `manual_rack_limits_are_profile_data_not_constants` | non-negotiable 1: two profiles of one airframe judge the same blueprint differently; a paired 6-position loadout is refused by the 4-position rack and accepted by the 6-position one |
| `an_unmeasured_limit_refuses_instead_of_reading_as_no_limit` | an unmeasured ceiling is `UnknownLimit`, while a profile that measures it accepts the same blueprint |
| `unknown_prices_and_footprints_are_refused_by_name` | `UnknownMass`, `UnknownCost`, `NotPriced`, `UnknownGunPositions`, `AirframeMismatch` and `Overflow` each by name |
| `blueprint_is_a_validated_typed_input` | kind checks, repeated-node refusal naming the slot, zero-position refusal, non-airframe profile, duplicate mask, duplicate quote, and an empty book refusing rather than pricing a loadout free |
| `budget_vocabulary_is_closed_and_consistent` | the six categories fold in `ALL` order, both `from_label` tables round-trip, each breach names one quantity, and the fixtures are `SyntheticFixture` rather than original |

## Mutation probes

Each mutation was applied to `construction.rs`, the `accept_f44_a_budget_bounds`
target run, and the file restored:

| Mutation | Failing tests |
| --- | --- |
| mass compared with `>=` instead of `>` (at-limit rejected) | 5 |
| gun positions counted as 1 per gun, ignoring the declared pair cost | 1 |
| unmeasured gun-position count defaulted to `1` | 1 |
| `checked_add` replaced with `wrapping_add` on weights | 1 |
| an unquoted component treated as free instead of refused | 2 |

## Designed vocabulary, not original data

Everything load-bearing here is newly authored project design carrying
`Origin::SyntheticFixture` and designed provenance. The original component
catalog, its masses, its prices, its per-airframe racks, its weight unit's
scale and its budget limits are **all unmeasured** — this stage read no
`CS_GAME_DIR` content, and F44's "Research boundary" plus the sheet's F44-D
stage own that audit. The fixture's masses and prices are chosen so the totals
land on round boundaries (6040 weight units, 42000 minor units), **not** to
resemble a plausible price list; the missile at 4250 against a 1500 gun is
arithmetic, not economics. Nothing here claims `verified_original`.

## Recorded unknowns (not guessed)

- **Whether a custom paint job carries a purchase price.** Unknown, so
  `BudgetCategory` has no paint row and paint contributes nothing to the
  budget. This is a declared gap rather than a silent zero: the closed
  six-category list makes an omitted category visible, and F44-D must resolve
  it before a purchase screen can be called complete.
- **The original's weight unit and money scale.** `WeightUnits` is a game-weight
  count and `DisplayMapping::try_new` takes its divisor as declared data
  precisely because neither conversion has been measured.
- **Per-airframe racks.** Only the manual's observed four positions and eight
  hardpoints are known, and the sheet requires confirming them per profile
  before treating them as universal. No airframe's real rack is declared here.
- **Paired-gun compatibility and which pairs exist.** Delegated to F44-B; this
  stage counts positions and decides nothing about legality.
- **Whether the weight limit and the price limit are independent in the
  original.** The two boundaries are independent here because they are two
  declared fields; whether the original couples them is unmeasured.
- **Availability, ownership and host-banned components.** Not modelled at this
  stage; F44-B's availability check and F44-C's import check own them (sheet
  AC02 and AC03).

## Not claimed

No original-data verification, no validator (the constraint rule set that
returns warnings and a normalized performance preview), no transactional
purchase/sell draft, no currency balance, no availability or banned-component
check, no construction/paint/loadout UI, no profile persistence. Those are
F44-B, F44-C and F44-D. This task awards at most **checked** status.