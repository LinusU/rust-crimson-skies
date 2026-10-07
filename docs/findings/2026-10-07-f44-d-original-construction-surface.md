# F44-D: original component availability, budgets and every stock blueprint

Date: 2026-10-07. Task: F44-D (#183)
(`specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
`### F44-D`). Contract: `docs/contracts/STATE-TRANSACTIONS.md`. Capabilities:
`retail` (read access to `$CS_GAME_DIR`), plus ordinary build/test.

`retail` here is **read access to the original files**. It is not evidence that
the original executable ran, and nothing below claims original *behaviour*:
what is measured is file content — ids, counts, callbacks and code shapes.

## Files and the one observable failure

- `crates/cs_content/src/construction.rs`: the original construction surface —
  `OriginalBudgetRow`, `ORIGINAL_BUDGET_ROWS`, `ORIGINAL_BUDGET_TOTALS`,
  `ORIGINAL_PURCHASE_REFUSALS`, `ORIGINAL_ARMOR_ZONES`,
  `ORIGINAL_ARMOR_ZONE_COUNT`, `ORIGINAL_PLANE_SLOTS`,
  `ORIGINAL_CONSTRUCTION_FIELDS`, `normalized_budget_word`,
  `original_budget_words` and `original_budget_vocabulary_gaps`; the three
  `ORIGINAL_*_SLOTS` counts are re-exported from `cs_content::weapons` so each
  number keeps one home.
- `crates/cs_app/src/construction/spawn.rs` (new): the AC04 harness —
  `NormalizedAircraft`, `preview_normalized`, `SpawnedBlueprint`,
  `SpawnedAircraft`, `BlueprintSpawnRequest`, `BlueprintSpawnError`,
  `PreviewRefusal` and `spawn_blueprint`.
- `crates/cs_app/src/construction/mod.rs`: `pub mod spawn` and its re-exports.
- `crates/cs_app/tests/accept_f44_d_spawned_aircraft_equality.rs`: six tests.
- `crates/cs_content/tests/accept_f44_d_retail_construction_surface.rs`: seven
  retail tests.
- `crates/cs_content/tests/f44_d_support/mod.rs`: the one reader both the
  retail suite and the evidence harness include.
- `crates/cs_content/tests/evidence_report_f44_d.rs`: the evidence harness
  (not named `accept_f44_d_*`).

Observable failure before this change: **nothing could spawn a blueprint.**
`AircraftBlueprint` was consumed only by the construction session and the
purchase weight check, so AC04's "actual spawned aircraft" had no
implementation at all — a preview could be drawn and a draft committed while
no aircraft existed to compare it with. The stage's second gap was
verification: no test re-measured the original's construction screens, so
F44-A's `BudgetCategory`, F44-A/B's refusal vocabulary and the manual's "four
gun positions" were fixture-only claims.

## Measured from the installation (ids, counts and code shapes)

Read through `cs_assets`' production ROF mount from members of
`GOSDATA/ASSETS/crimson.rof`, re-measured on every run by the retail suite:

1. **The purchase screen's budget table** (`ASSETS/SCRIPTS/PURCHASE.SCRIPT`):
   five weight-and-cost rows — `airframe`, `engine`, `armor`, `guns`,
   `hardpoints` — plus a `totals` row, each a set of `@ctl@PE` controls whose
   `YC` label ids spell it, and each cell filled by an **engine callback**
   (2251/2252 in one call for airframe and engine, `TJ = 2254…2261` for the
   list rows, 2262 for totals). The callback is the measurement that matters:
   the *shape* of the budget is in a file and the *numbers* are not.
2. **The purchase refusals** (`ASSETS/SCRIPTS/RESOURCE.H`): `IDS_PX_PUR_PROBLEM
   1182`, `IDS_PX_PUR_NOENGINE 1183`, `IDS_PX_PUR_NOPAINT 1225`,
   `IDS_PX_PUR_INSUFFICIENT 1226`, `IDS_PX_PUR_OVERWEIGHT 1227`. The purchase
   button's liveness comes from one callback
   (`if (!callback($$E$$, 2264, AQA.BC)) mail (10000, BQA)`).
   `OVERWEIGHT` and `INSUFFICIENT` corroborate `CampaignError`'s
   `LoadoutOverweight` and `InsufficientFunds`; the other three name a rule
   whose original *condition* is unmeasured.
3. **Four armor zones, named twice**: `ARMOR.SCRIPT` builds `object X[4]` under
   `ar_t_nosetitle`/`ar_t_tailtitle`/`ar_t_lefttitle`/`ar_t_righttitle`, and the
   header declares `IDS_AR_NOSE_TITLE 1044`, `IDS_AR_TAIL_TITLE 1045`,
   `IDS_AR_LEFT_TITLE 1046`, `IDS_AR_RIGHT_TITLE 1047`.
4. **Four gun positions on two independent screens**: `GUNS.SCRIPT`
   (`object DS[4]`, `object ES[4]`, `int AS[4]`, each `for (R=0; R < 4; R++)`)
   and `ORDINANCELAYOUT.SCRIPT` (`object PKA[4]`, `object QKA[4]`). This is
   non-negotiable 1's "four gun positions" measured from code rather than from
   the manual alone.
5. **Eight rocket slots and two hardpoint points**: `object RKA[8]` with
   `for (BA = 0; BA < 8; BA++)`, and `object DT[2]` with three `… < 2 …` loops.
6. **Four saved-plane slots**: `PLANECONSTRUCTION.SCRIPT` builds `object HMA[4]`
   filled by `callback($$E$$, 2243, (R), HMA[R].AK, 1)` for `R` in `0..4`.
7. **The construction screen's own comparisons**: `px_t_weightcapacity`,
   `px_t_currentweight`, `px_t_planecost`, `px_t_cashtitle` — a weight
   *capacity* against a *current* weight and a plane cost against the player's
   cash, which is exactly the pair `ConstructionRules::max_mass`/
   `BlueprintTotals::mass` and `max_cost`/`totals.cost()` answer between.

**A count is an array size or a loop bound, never a control argument.**
`@globals@AR` precedes every `@ctl@PM` control and takes the values `5`, `7`,
`11`, `12`, `13`, `26`, `27`, `32` across screens; it is a control parameter
whose meaning is unmeasured. Reading it as "how many entries the list holds"
would have reported eleven airframes and seven engines from a scroll height.
`f44_d_support::COUNT_BOUNDS` contains no `@globals@AR` entry, and the retail
suite says so in a doc comment so the mistake cannot creep back in.

## The budget vocabulary audit

`original_budget_vocabulary_gaps()` compares `BudgetCategory::ALL` with
`ORIGINAL_BUDGET_ROWS` as **noun sets** under one documented normalization
(lowercase, one trailing `s` removed — `guns` and `gun` are the same word, and
no two different nouns collide). The retail test derives the original side
*from the file* (every `pur_t_*` id ending in `weight` or `cost`) and asserts
the derived set equals the committed rows, so the audit's input is the
installation's own row set rather than a restatement.

Result, exactly three gaps:

| side | noun | reading |
| --- | --- | --- |
| ours | `ordnance` | rockets have no purchase weight/cost row of that noun; the screen's eight rocket slots are chosen on `ORDINANCELAYOUT.SCRIPT`, not on the purchase table |
| ours | `equipment` | hardpoint equipment has no purchase row either |
| original | `hardpoint` | the screen prices two hardpoint points; this project prices no category by that noun |

`airframe`, `engine`, `armor` and `guns` are shared and are **not** reported.

This is a finding, not a defect on either side: which of the two the original
really charges for is unmeasured (the numbers live in the executable), so the
audit reports the disagreement by name instead of renaming a category to make
it disappear. Nothing in the validator, the economy or the spec changed.

## AC04: preview and spawned aircraft

`spawn_blueprint` is the missing half, and it is deliberately ordered so that
every refusal happens before the world changes:

1. `ConstructionRules::validate` — the same validator the preview and the
   commit run, so a spawn cannot meet a weaker rule.
2. The declared gun catalogue is matched per fitment by `(gun, mount)`; an
   uncovered fitment is `UndeclaredGun`, never a dropped gun.
3. The `GunBank` is built from those mounts.
4. `spawn_flight_body` creates the body through the production physics path.
5. `WeaponSession::register` makes the guns fireable; a refusal despawns the
   body again, so a failed spawn still leaves no entity.
6. `SpawnedBlueprint` is written from what the session reported.

`preview_normalized` reads `ConstructionScreen::view`; `SpawnedAircraft::normalized`
reads the component **back out of the world**. Both produce a
`NormalizedAircraft { mass, weapons, paint }` where `weapons` is sorted by
mount, so a preview read in authored order and a spawn read in registration
order compare equal when they hold the same mounts.

The comparisons are stated in **game-weight units**, the unit both sides share.
They are not converted to kilograms: see the unknowns below.

### The tests

`accept_f44_d_preview_and_spawned_aircraft_have_equal_mass_weapons_and_paint`
is the minimum scenario. Around it:

- `accept_f44_d_a_spawn_of_a_different_record_is_detected_as_unequal` proves the
  comparison is discriminating: the preview shows the edited draft, the spawn is
  handed the stale pre-edit record, and **all three** projections disagree. A
  harness whose two sides read one value would fail this test by construction.
- `accept_f44_d_an_invalid_draft_is_refused_by_both_the_preview_and_the_spawn`
  pins the failure case: one unit over the ceiling, both sides refuse, and the
  world holds no `SpawnedBlueprint`.
- `accept_f44_d_a_gun_no_declared_record_covers_refuses_the_spawn` pins the
  "refused, never dropped" half: the blueprint is valid, only the declared
  record is missing, and the spawn names `gun_mount_2` rather than omitting it.
- `accept_f44_d_the_spawned_body_carries_the_declared_total_mass` asserts the
  one-mass rule on the spawned `FlightAircraft`.
- `accept_f44_d_the_spawn_bank_is_the_blueprints_own_mounts` reads the bank
  back from the weapon session the spawn registered with and checks it against
  the blueprint's own mounts — the bank never reaches the caller, so the session
  is the only place it can be observed.
- Both refusal cases additionally assert that nothing became fireable: a
  refused spawn registers no guns with the session.

Selection: `cargo test --workspace --locked -- accept_f44_d_ --include-ignored`
runs **13** tests — six in `cs_app` (AC04 and its failure cases) and seven in
`cs_content` (the retail re-measurement) — all pass with `CS_GAME_DIR` set.

## Recorded unknowns (not guessed)

Each of these is stated in full — with its affected content and its resolving
task — in the committed evidence report's `review.method`, in this finding and
in the filed follow-up task, so it survives this task being marked done; the
report's `unknowns` array is empty because every measurement *it* made
resolved, which is what `tools/validate_evidence.py --require-pass` checks
(the same split F27-D, F28-D and F39-D record). Every fidelity,
`verified_original` and release claim for F44 stays gated on resolving task
#563.

1. **The weight unit's conversion to SI is unmeasured.** No file declares a
   component's mass, so `WeightUnits` is never converted to the kilograms the
   physics body integrates. AC04's mass equality is therefore stated in
   game-weight units, and the spawned body's kilograms are asserted only
   against the one-mass rule. The kilograms in that test are declared fixture
   values, labelled as such. *Resolving task: #563.*
2. **Every component's mass and price, and each airframe's weight and cost
   ceilings, are engine callbacks.** `ohardpointweight`/`ohardpointcost` are
   engine-dictionary names whose numbers are in the executable, not in a file
   (F28-D recorded the same for the hardpoints). `PriceBook` therefore still
   carries no original quote, and no retail `ConstructionRules` ceiling exists.
   *Resolving task: #563 (and #452 for the ordnance half).*
3. **No shipped file enumerates the aircraft a profile starts with.** "Every
   stock blueprint" is bounded at the construction screen's four saved-plane
   slots; their content is unmeasured. *Resolving task: #563.*
4. **Which damage-graph node each armor zone maps to is unmeasured**, so
   `ArmorFitment`'s `DamageNodeKey` stays declared per profile rather than
   bound to `nose`/`tail`/`left`/`right`. *Resolving task: #563.*
5. **`PaintSelection` → `PaintChoice` is unmeasured.** The spawned aircraft
   carries paint *references* (non-negotiable 5: never pixels), but composing
   them through `LiveryRuntime` needs a mapping no file declares. *Resolving
   task: #563.*
6. **The three unnamed purchase refusals** (`NOENGINE`, `NOPAINT`, `PROBLEM`)
   have ids but no measured condition, so no validator rule was added for them.
   *Resolving task: #563.*

## Follow-ups (not this task)

Recorded as a note on the existing research task #563, *"Locate where the
original states airframe engines, armor, guns, ammunition and hardpoints"*,
which owns exactly this question — this stage adds its measured starting point
and its six unresolved items rather than opening a duplicate. #563 is the
resolving task named by every unknown above; until it closes, F44's
`verified_original` and release claims stay gated. No unrelated bug was found
in this stage's scope.

## Limits of this stage

A code/test pass awards at most **checked**. `retail` here proves file access
and file content; it never proves how the original behaves, and no agent ran
the original executable. The committed evidence claims `implemented` only.
