# F49-D.03: the retail InstantActionCatalog and AC01

Date: 2026-10-18. Task: F49-D.03 (#785). Capability: **`retail`** (read-only
`$CS_GAME_DIR`). Nothing was run, rendered or played; the claim stays
`implemented`. `gpu` and `audio` were available and not used.

## What was built

`cs_app::ui::instant_action::retail_instant_action_catalog` (new
`crates/cs_app/src/ui/instant_action/retail.rs`) assembles the catalog from
installation data through production readers:

* identities, offered mission types and declared aircraft: the F49-D.01 survey
  (30 presets over 8 scenarios);
* plane display name -> airframe id: `cs_app::mission_start::airframe_entry`
  (called, not edited), checked against the F49-D.02 option table;
* world: `world/<group>` when the F14-D.3 world rows hold the directory's group
  (all eight do), with the `ia.zrd` span as provenance;
* environment: the `weather.zrd` the **IA archive itself** carries, read by
  `read_mission_weather` (all eight bind); `Unknown` with the reader's message
  otherwise;
* options: F49-D.02's table through the new
  `ScenarioOptionTable::validate_partial`, which tolerates an empty list only
  for a dimension recorded `Unmeasured` (loadouts, pilots) and still refuses an
  empty *measured* list;
* every preset: `Origin::Installation` with a span over the `ia.zrd` member, so
  `is_original_complete()` is true.

The measured roster per preset: one player seat (airframe from `player_plane`),
`num_wingmen` ally slots, one enemy actor per `group<N>` (plane, skill tier,
authored count, name label) and the named ace (plane, skill, label). Groups are
**not** expanded by count: the totals reach 18 aircraft, over the per-side
bound, and the descriptor does not say every authored aircraft is placed.

## Schema changes in the owner path (and why they weaken nothing)

* `ScenarioParameters.difficulty` / `.rules` are `Declared<T>` (declared, or
  `Unmeasured` with claim and reason). `ScenarioParameters::new` still takes
  declared values; `new_unmeasured_rules` builds the retail form. `lower_preset`
  refuses an unmeasured value with `LowerError::UnknownValue` naming
  `scenario difficulty` / `scenario victory rules`.
* `ScenarioActorSpec.faction` is `Resolved<ContentId>` (`try_new` unchanged for
  a known faction; `try_new_resolved` takes an unknown one) and gains `skill`,
  `authored_count`, `label`, `role`.
* `InstantActionPreset` gains `original_mission_type` and
  `unmeasured_facts` (a fact the roster cannot express; `lower_preset` refuses a
  preset carrying one).
* `InstantActionCatalog::try_new` refuses two presets that launch the same
  scenario **and** the same (or no) mission type; presets of one scenario that
  differ by mission type coexist. Without this, 30 presets over 8 scenarios
  cannot be held. `preset_for_scenario` keeps its first-in-id-order meaning;
  `presets_for_scenario` lists all.
* Customizing a preset leaves an unmeasured difficulty/rules unset on the draft
  (the draft reports them in `unset_dimensions`) and a first
  `IaEdit::Difficulty` starts from no overrides.
* `lower_preset_roster` exposes the measured part of a roster for AC01's
  expected-actors half.

Existing F49-A/B tests: only mechanical accessor edits (`.rules()` ->
`.rules().declared().expect(..)`, `.difficulty()` likewise, `.faction()` ->
`.faction_id()`) in `accept_f49_a_preset_spawn.rs` and
`accept_f49_b_scenario_normalization.rs`, plus one lib test in
`instant_action.rs`. No assertion was weakened, skipped or deleted.

## AC01, as far as the data allows

* **Actors (verified)**: for all 30 presets the lowered roster equals the
  descriptor's (player plane, wingman count, group planes/skills/counts, ace),
  checked against `AIRFRAME_TABLE` directly.
* **World (verified)**: `world/<group>` for all eight, environment equal to the
  weather reader's id and span.
* **Rules, difficulty, factions, loadouts, survivability, titles: unmeasured.**
  No retail preset launches; `lower_preset` refuses each, in order, with the
  claim and reason (`f49.d.03.unmeasured.*`). The launch half of AC01 is
  therefore **unmet** and stays so until the follow-ups below resolve it.

## Unknowns (recorded, not guessed)

* Faction/allegiance and survivability of every actor (#1582).
* Victory rules per mission type (`dogfight_ace`, `dogfight_squadron`,
  `zeppelin_run`, `stunt_flying`), per-scenario difficulty, which declared
  aircraft each type spawns, wingman planes, and `ZBD/c2b/IA1`'s `player_plane`
  and `num_wingmen` (#1583). c2b's missing wingman count is a preset fact that
  refuses launch; its player airframe is `Unknown`.
* Preset titles and whether the menu offers every (scenario, type) pair (#1584).
* Loadout identities (#1575, existing).
* The ace has no side key in the descriptor; it is seated on the enemy side as
  an **inference** from the key set and mission-type name, not a measurement.
* The seed is the only designed value: FNV-1a of the preset id mixed with
  `SCENARIO_SEED_DOMAIN` (`preset_seed`), explicit and unique per preset.

Evidence: `docs/findings/evidence/F49-D.03.json`. Sources: the installation's
eight IA `zrdr.zbd` archives (`ia.zrd`, `weather.zrd`), the F14-D.3 world rows,
the executable's airframe table as transcribed in `mission_start.rs`.
