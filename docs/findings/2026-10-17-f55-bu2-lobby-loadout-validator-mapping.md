# F55-BU2: a production lobby `LoadoutValidator` needs an owner decision — the mapping is blocked on two gaps

Date: 2026-10-17. Task: #764 `F55-BU2-VALIDATOR` "Production LoadoutValidator
for the lobby over cs_content construction" (follow-up of #221 / F55-B, whose
finding `docs/findings/2026-10-08-f55-b-host-validation-readiness-membership.md`
recorded that the hook had only test doubles). Ordinary build/test only; no
capability was needed and none was used.

Status: **blocked.** A judgement over the shared construction rules *given an
authored blueprint* is implemented and tested
(`crates/cs_content/src/lobby.rs`, 16 tests,
`cs_app::ui::lobby::ConstructionLoadoutValidator`), but it cannot be fed a real
lobby loadout honestly, and the task says to `block_task` "if it needs an owner
decision (for example where the authored blueprint table lives)". Two gaps make
that decision necessary. Both are measured here; neither is guessed around.

## What is implemented (and what it proves)

`cs_content::lobby::LobbyConstructionCatalog` owns authored
`AircraftBlueprint`s, their airframes' `ConstructionRules`, a
`ConstructionPolicy` and a `PriceBook`, and `judge` lowers
`ConstructionRules::validate` to one stable code per refusal
(`unknown_blueprint`, `unknown_rules`, `loadout_mismatch`, `unmeasured_budget`,
`unknown_pairing_rule`, `over_mass`, `over_cost`, `over_gun_positions`,
`over_hardpoints`, `banned_component`, `unavailable_component`,
`unsupported_gun_selection`, `gun_not_pairable`).
`cs_app::ui::lobby::ConstructionLoadoutValidator` is the `LoadoutValidator`
adapter over it. The 16 `accept_f55_bu2_*` tests call the production `judge`
and exercise every code; they fail to compile if the module is removed.

That is the half the task's first "Done when" bullet describes, and it holds
*for a loadout whose component ids are construction ids*.

## Gap 1 — no authored blueprint-id → `AircraftBlueprint` table

The task already recorded this (`BTreeMap<ContentId, AircraftBlueprint>` has no
hits in `cs_content`). Re-checked: across `crates/` the only two paths that turn
ids into construction fitments are

* `cs_content::legacy_import::blueprint_from_record` — a **legacy record**
  read through a declared `LegacyLayout` + `BlueprintFieldMap`, i.e. original
  bytes, not a lobby loadout; and
* the in-memory `AircraftBlueprint` an F44-C `ConstructionSession` edits.

Nothing maps a `blueprint` catalog id to an `AircraftBlueprint`. The module
therefore takes the table as an injected `LobbyConstructionCatalog` and refuses
a blueprint it does not hold. **Where that authored table is provisioned from
the installation is not measured** (F44-D records the original budget rows as
unmeasured; F14-D.10 records the component rows as largely absent), so it is an
owner decision, not a value to invent.

## Gap 2 — the lobby's component vocabulary is disjoint from construction's

This is the gap that makes even an authored table insufficient, and it was not
recorded in the task.

| layer | vocabulary | evidence |
| --- | --- | --- |
| lobby `Loadout.components` | `Airframe`, `Engine`, `Armor`, **`Gun`**, **`Ammo`**, `HardpointEquipment` | `cs_net/src/lobby.rs:402` `is_component_kind` (no `Weapon`); used by `check_loadout` (`lobby.rs:1534`) and F58-B's `check_loadout_shape_and_bans` (`cs_net/src/validation.rs:1423`) |
| construction guns/ordnance | **`Weapon`** only | `cs_content/src/construction.rs:505` (`GunFitment::try_new` requires `ContentKind::Weapon`), `:590` (`OrdnanceFitment::try_new` requires `ContentKind::Weapon`) |

`ContentId` identity is `namespace/key` (`cs_types/src/content.rs:716-737`), so
`gun/wep_03` and `weapon/wep_03` are different ids. Consequences, both measured:

1. A lobby loadout naming a construction gun (`Weapon`) is refused by the
   lobby's **own shape check** as `LoadoutProblem::Malformed`
   (`is_component_kind(Weapon) == false`), before any validator sees it. So a
   real member loadout can never carry a construction gun id.
2. A lobby loadout naming the lobby's own `Gun`/`Ammo` ids can never equal an
   authored blueprint's fitted components, so the `loadout_mismatch` rule
   refuses it. The lobby loadout is also a **set** (a duplicate component is
   `Malformed`, `lobby.rs:1532`), while an authored blueprint's component list
   is a **multiset** (the fixture fits four `Weapon` guns, two plates, four
   missiles).

Reproduction (the integration test wired `ConstructionLoadoutValidator` into
`LobbyScreen::submit_host` / `LobbyLink` and handed it the fixture boundary
blueprint's own component set — an empty component list and the fixture ids):

```
left:  Domain(LoadoutRejected(Malformed))
right: Domain(LoadoutRejected(Invalid { code: "loadout_mismatch" }))
```

`Gun`/`Ammo` are the lobby's own *designed* component kinds (F55-A finding:
"the ban component kinds here are designed"); `Weapon` is construction's own
designed kind (F44-A). The two were never reconciled, and no bridge is declared
anywhere. Inventing one (by key, by kind, or by position) is a guess about the
original catalog, which AGENTS rule 4 forbids; and the underlying component
rows themselves are largely unmeasured (F14-D.10: armor, ammunition and
hardpoint rows do not exist; guns are `weapons.zrd` `CANNON` records with no
`Weapon`-layer binding).

## The decision the owner must make

Either (a) declare the lobby's component vocabulary the construction one (for
example extend `cs_net::lobby::is_component_kind` to the construction kinds, or
declare how a lobby loadout names a fitment), or (b) declare a designed bridge
between the two vocabularies (and state its evidence), or (c) declare that the
lobby loadout is only a *blueprint reference* and the flat component list is
judged by the lobby's own shape/ban rules alone, not by construction. Each is a
semantics decision about original content, so it is the owner's, not an
implementer's.

## Resolving work

* The owner's decision above, recorded in the F55 sheet or a contract.
* Then: provision `LobbyConstructionCatalog` from the authored content the
  application holds, and wire `ConstructionLoadoutValidator` into
  `LobbyScreen::submit_host` / `LobbyLink`.
* Independently: F44-D's measured component masses/prices, and F14-D.10's
  missing armor/ammunition/hardpoint rows.

## Resolution (owner decision 2026-10-17, task #764)

Option (a), one vocabulary. `cs_net::lobby::is_component_kind` now also accepts
`ContentKind::Weapon`, the kind construction fits guns and ordnance as; the
F58-B shape check (`check_loadout_shape_and_bans`) uses the same function, so
it is consistent. `Gun`/`Ammo` stay accepted by the shape check; a loadout that
names them cannot equal an authored blueprint's components and is judged
`loadout_mismatch`. No key-, kind- or position-based translation exists.
The content-side module is `cs_content::lobby_loadout` (`cs_content::lobby`
was taken by the F55-CU2 scenario menu).

Tests: `accept_f55_bu2_lobby_*` in `crates/cs_app/tests/accept_f55_bu2_lobby_validator.rs`
drive `ConstructionLoadoutValidator` through `LobbyScreen::submit_host` /
`LobbyLink`: the fixture blueprint's component set is accepted, an unknown
blueprint is `Invalid{unknown_blueprint}`, a `Gun`-kind id is `Invalid{loadout_mismatch}`.

### Unknown (Gap 1, unchanged)

The authored blueprint-id -> `AircraftBlueprint` table for an installation is
not provisioned. The catalog is injected and a blueprint it does not hold is
refused `unknown_blueprint`. Resolving task: F44-D (unmeasured). Nothing is
claimed about retail blueprints.
