# F64-B: verified read-only import subset — retail measurements and the blueprint validation path

Date: 2026-10-07. Task: F64-B "Implement verified read-only import subset"
(`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, section
`### F64-B`). Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Evidence report: `docs/findings/evidence/F64-B.json` (validated with
`tools/validate_evidence.py --require-pass`).

Capabilities used: **`retail`** (read-only reads of `$CS_GAME_DIR`) plus
ordinary build/test. The original executable was never run; nothing here is a
claim about original runtime behaviour or about any original file's byte
layout, because the measurement's central finding is that **no legacy file of
any import class ships with the installation**.

## What the retail measurement found

Three findings, each re-derivable from `import-surface.json` in the evidence
bundle:

1. **No legacy artifact ships.** All 229 inventoried files were checked
   against the storage shapes the engine itself names — `SavedGames`,
   `Planes\`, `.sav`, `Status.dat`, `Mission.`, `Persist.` — with **zero**
   matches. Legacy saves and custom planes are runtime-created files; the
   install cannot prove their formats because it contains none.
2. **The storage locations are measured anyway.** The owner-supplied
   decrypted engine image (`crimson.decrypted.exe`, the owner's decryption of
   `crimson.icd`, SHA-256 recorded in the evidence) holds the path templates
   the engine formats at runtime: `Planes\*.*` and `Planes\%s` (one saved
   plane per name, beside `rb` for reading and `wb+` plus a bare `Planes`
     for creating), `SavedGames\%s\AutoSave.sav`, `SavedGames\%s\*.sav`,
   `SavedGames\%s\%s`, `%s\Status.dat`, `%s\Mission.%1d%02d`,
   `%s\Persist.%1d%02d`, `%s\%s.sav`, and the registry key
   `SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\1.0`. These are
   *locations and file-name shapes only* — what is inside any of those files
   is unmeasured and stays `Unknown` in the inventory.
3. **Custom aircraft are referenced by original content.**
   `ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT` declares `object HMA[4]`, labels
   each slot from a stored plane (`HMA[R].YC = "px_p_plane" conv$(R)`) and
   fills slot `R` through engine callback `2243`. That is the measured
   original content path the spec's "required when an original content path
   references it" names, so `LEGACY_LAYOUT_INVENTORY[CustomAircraft]` now
   carries `referenced_by: &["ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT"]` and
   `is_required()` is true. Two F64-A acceptance tests pinned the
   pre-measurement state ("not required **until** a path references it") and
   were updated to assert the now-measured row; every save class stays a
   separately labeled optional enhancement, and `CustomLoadout` keeps an
   empty `referenced_by` because whether a loadout lives inside the plane
   file or separately is still unmeasured.

## What the implementation adds

`crates/cs_content/src/legacy_import.rs` gains the blueprint subset the task
history asked for — extending `ImportedRecord::resolved_ids` into real
`AircraftBlueprint`s and judging them with F44's rules rather than a second
budget implementation:

- **`BlueprintFieldMap`** (`BlueprintFieldSlot`, `BlueprintRole`) is the same
  kind of declared, evidence-gated data `LegacyLayout` is: which record
  fields are the airframe, the engine, each gun (with its `DamageNodeKey`
  mount and a `Resolved<u32>` position count) and each ordnance item (with
  its hardpoint). A map must carry exactly one airframe and one engine role,
  and `validate_against(layout)` refuses a field the layout does not
  declare, a slot that is not an id slot, and a role whose id class
  disagrees with the layout's declared class — all before any record is
  read. Fields the map does not name (a name, paint, a checksum) are not
  blueprint roles and stay uninterpreted.
- **`assess_imported_blueprints(&BlueprintImportRequest)`** is a pure
  function with the same read-only shape as `plan_import`: shared references
  only, no handle, no writer, no host path. For each record it resolves the
  mapped fields through the *same* `LegacyIdMap`/`Catalog` pair (so an
  unmapped component is the same `UnresolvedRow` the plan reports), assembles
  the blueprint through `AircraftBlueprint::try_new`, and calls the
  production `ConstructionRules::validate` — never a parallel budget —
  yielding `BlueprintRecordOutcome::{Conforming, Rejected, Refused}` per
  record in a `BlueprintImportReport`.
- **AC02** is the `Rejected` outcome: the retained `BlueprintVerdict` carries
  the exact `LimitBreach` fields (`Mass { limit, total }`,
  `GunPositions { limit, used }`, `RocketHardpoints { limit, used }`,
  `Cost { limit, total }`) and every `ConstraintViolation`, so a violating
  imported blueprint is rejected with specific machine-matchable fields
  rather than a prose message.
- **`synthetic_blueprint_layout()`** (cs_formats) and
  **`synthetic_blueprint_map()`** (cs_content) are `ClaimStatus::Designed`
  fixtures — a record shape with airframe/engine/4 guns/2 rockets mirroring
  the original construction screen's measured slot counts — admitted only
  through `LayoutAdmission::AllowDesignedFixtures`, never claims about a
  stored-plane format.
- The record's blueprint id is derived as
  `blueprint/legacy.<sanitized layout id>.<record index>` — deterministic and
  source-identifying within a document; cross-file uniqueness is F64-C's
  concern.
- Armor, equipment and paint are left empty on imported blueprints: how a
  legacy file encodes them is unmeasured, so they are not guessed. The stock
  rules measure what is declared.

The plan and the blueprint stage agree: a document `plan_import` classes
`Full` assesses into conforming blueprints through the same resolved
identities (test `accept_f64_b_the_blueprint_stage_agrees_with_the_import_plan`).

## Deliberately not done

- **No `ui/import.rs`.** The task's owner paths name it, but no import UI
  module exists in `cs_app` and F64-C's description owns wiring "the
  implemented path into its actual producer and consumer". Adding a UI shell
  here would be unwired product code.
- **No save-format claims.** `crimson.icd`'s `.text` is encrypted and no
  save/plane file exists to read; string adjacency is recorded as
  `ObservedTool`-grade evidence (a tool observed the bytes), never as
  verified layout.
- **No loadout requirement flip.** `CustomLoadout.referenced_by` stays
  empty: the same screen *might* store the loadout inside the plane file,
  and "might" is exactly what `referenced_by` exists to not record.

## Follow-ups to file

- The stored-plane/save byte layouts need an original run or an
  owner-captured file (`Planes\*`, `SavedGames\<profile>\*`): no agent can
  synthesize one.
- Whether a stored loadout is inside the plane file or separate.
- Armor/equipment/paint/name blueprint roles and a `BlueprintFieldMap`
  measured from a real file.
