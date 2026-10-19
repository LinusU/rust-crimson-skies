# F49-D.02-FU-PILOT: the Instant Action pilot identities (`ScenarioOptions.pilots`)

Date: 2026-10-18. Task: #1576. Follow-up of F49-D.02 (#784). Capability:
**`retail`**, read-only `$CS_GAME_DIR`. Nothing was run, rendered or played; no
claim is raised beyond `implemented`.

## Measured

The only pilot identities the installation stores that an Instant Action
scenario names are the **aces** each `ZBD/<group>/IA1/zrdr.zbd` `ia.zrd`
declares (`ace_name`, with `ace_plane` and `ace_skill`), read through the
production survey `survey_retail_instant_action`
(`IaDirectory::aircraft().ace()`):

| directory | `ace_name` label | `ace_plane` |
| --- | --- | --- |
| c1/IA1 | `MSG_PALBLAKE_NAME` | Peacemaker |
| c1b/IA1 | `MSG_BSWAN_NAME` | Fury |
| c1c/IA1 | `MSG_GKHAN_NAME` | Firebrand |
| c2/IA1 | `MSG_HHUGHES_NAME` | Bloodhawk |
| c2b/IA1 | `MSG_GKHAN_NAME` | Devastator |
| c3/IA1 | `MSG_JPEROT_NAME` | Kestrel |
| c4/IA1 | `MSG_BREDMANN_NAME` | Bloodhawk |
| c5/IA1 | `MSG_SSCRAWFORD_NAME` | Peacemaker |

All eight declare `ace_skill = ace`. That is **8 aces over 7 distinct pilots**:
`MSG_GKHAN_NAME` is the same pilot flying a different plane in c1c and c2b.

`IaDenominator::ace_pilot_ids` returns the distinct labels as
`pilot/<lowercase label>` ids in id order (`pilot/msg_palblake_name`, ...). The
id key is the original's own localized-string label, not an invented name.
`retail_scenario_options` now surveys the installation and passes those ids to
`retail_scenario_options_with_pilots`; the `pilots` dimension is tagged
**measured** with reader `PILOT_OPTION_READER`. The retail table's remaining
empty required list is `loadouts` alone.

## Not measured (recorded, not guessed)

* Whether the original's Instant Action screen lets a player **choose** a pilot.
  The aces are fixed per scenario descriptor; no `ia.zrd` key lists a selectable
  pilot set. The list is the pilot identities the original stores, not a
  proven menu.
* The display text of each label (the string table is not resolved here).
* The wingmen and the four enemy groups carry plane/skill/count but **no pilot
  name** in `ia.zrd`, so they contribute no identity.
* `ScenarioActorSpec` pilots for non-ace actors have no retail source.

## Wiring left outside this task's owner paths

`cs_app::ui::instant_action::retail` still calls
`retail_scenario_options_from_baseline`, which is given no pilots and keeps the
`pilots` list unmeasured. Switching it to `retail_scenario_options_with_pilots`
needs `crates/cs_app/src/ui/`, which this task does not own; a follow-up task
is filed. The cs_app acceptance test and evidence harness for F49-D.02 were
updated only where `retail_scenario_options` now returns pilots
(`["loadouts"]` instead of `["loadouts", "pilots"]`, and the pilot limitation
text).

## Wiring landed (#1630, 2026-10-19)

The follow-up task wired the measured pilots into the retail catalog:
`cs_app::ui::instant_action::retail::assemble_retail_catalog` now calls
`retail_scenario_options_with_pilots` with the survey's `ace_pilot_ids()`, so
the assembled `InstantActionCatalog`'s option table offers the seven
`pilot/<label>` ids and reports only `loadouts` and `relations` unmeasured.
Pinned by `accept_f49_d_03_retail_nothing_in_the_catalog_comes_from_the_fixture`
(the catalog's `pilots()` must equal an independent re-survey's
`ace_pilot_ids()`) and by `standin_catalog` in
`crates/cs_app/tests/accept_f49_d_04_validation_errors.rs`, which now widens
the retail table with stand-ins for the two remaining unmeasured lists only.
