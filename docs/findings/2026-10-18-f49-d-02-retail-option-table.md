# F49-D.02: the retail custom-scenario option table (`ScenarioOptions`)

Date: 2026-10-18. Task: #784 "Assemble the original custom-scenario option table
(ScenarioOptions) from measured collections" (stage D.02 of F49-D, parent #204).
Feature sheet: `specs/F49-instant-action-presets-and-custom-scenarios.md`.
Shared contract: `docs/contracts/UI-NETWORK.md` ("full option tables"); evidence
contract `docs/contracts/CLI-EVIDENCE.md`. Required capability: **`retail`**,
used read-only. `gpu` and `audio` were available and **not used**: nothing is
rendered or played, no original run happened, and nothing here claims
`verified_original`.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/instant_action.rs`: the retail option-table assembly —
  `ScenarioOptionDimension`, `OptionListEvidence`, `ScenarioOptionTable`,
  `retail_scenario_options` / `retail_scenario_options_from_baseline`, the
  per-reader and per-gap constants, and the new
  `ScenarioSchemaError::MissingOptionEvidence` variant.
- `crates/cs_content/tests/accept_f49_d_02_option_table.rs` (new): the synthetic
  failure cases through `ScenarioOptions::try_new` and `ScenarioOptionTable`, and
  the two retail tests.
- `crates/cs_app/tests/accept_f49_d_02_retail_option_table.rs` (new): the retail
  environment list read by the production weather reader, and the
  `airframe_entry` display-name -> airframe-row cross-check.
- This file; the evidence harness `crates/cs_app/tests/evidence_report_f49_d_02.rs`
  and the committed report `docs/findings/evidence/F49-D.02.json`.
- `crates/cs_content/tests/main.rs`: one `mod` line (wiring only). No protected
  path, no `Cargo.toml` and no `Cargo.lock` change.

**One observable failure:** before this work `ScenarioOptions` was only ever
assembled by the synthetic fixture (`synthetic_scenario_options`) and no
constructor read the installation's own world, faction or airframe rows. With
the constructor removed, `accept_f49_d_02_retail_every_measured_list_is_rederived_from_the_installation`
fails at the first assertion (the retail world list is unavailable); with only
the fixture lists supplied as the retail ones, the same test fails because the
fixture world/faction/airframe ids do not equal the installation's rows, and
`accept_f49_d_02_the_retail_table_is_not_the_fixture_table` fails directly.

## What the constructor assembles, and from where

The constructor reads the installation once (`cs_content::catalog::baseline::
retail_baseline`) and takes the environments the caller measured, then tags each
of the sheet's ten dimensions with the evidence class of its entries. Its output
is a [`ScenarioOptionTable`] — the table *before* validation — which keeps the
lists beside their provenance and turns into a validated `ScenarioOptions`
through `validate()`.

| dimension | evidence | source |
| --- | --- | --- |
| `worlds` | measured | the F14-D.3 `ContentKind::World` rows of `retail_baseline` (8 world groups) |
| `factions` | measured | the F14-D.5 `ContentKind::Faction` rows (11 paint patterns) |
| `airframes` | measured | the F14-D.6 `ContentKind::Airframe` rows (11 declared roots) |
| `difficulty_tiers` | measured | the F32-D measured steps (`DifficultyTier::measured_step`), three of the four declared tiers |
| `environments` | measured | the ids `cs_app::environment::retail::read_mission_weather` reads per mission (F19/F20); supplied by the caller because that reader lives above `cs_content` |
| `victory_conditions` | designed | F49-A's authored vocabulary, declared design with its rationale |
| `max_players` | designed | the module's own `MAX_SCENARIO_PLAYERS` bound, declared design so an oversized request is refused while the original's limit is unmeasured |
| `loadouts` | **unmeasured** | no producer — see below |
| `pilots` | **unmeasured** | no producer — see below |
| `relations` | **unmeasured** | no producer — see below |

Every list is recorded, never guessed. A dimension with no measured producer is
reported `Unmeasured` with a reason and a resolving task, and its list stays
empty, so `validate()` refuses the table with `EmptyOptionTable` naming it. The
empty axes of the owner's installation are `environments` (when no caller
supplies the weather reader's ids), `loadouts` and `pilots`.

### Why `loadouts`, `pilots` and `relations` are unmeasured (recorded, not filled)

The task names F27-D for loadouts and `cs_content::pilots` for pilots. Read
carefully, neither produces the `ContentKind::Loadout` / `ContentKind::Pilot`
**identity list** an option table selects from:

- F27-D measures *counts* and gun groups
  (`cs_content::weapons::OriginalGunLoadout`, `OriginalLoadoutCounts`,
  `cs_content::ordnance::OriginalOrdnanceSurface`) and F27-E imports ammunition
  identities into `ContentKind::Ammo` (`OriginalGunAmmunitionCatalogue`). No
  production reader mints a `ContentKind::Loadout` id, and one derived from a
  count would be a fabricated retail-looking id (AGENTS rule 4).
- `cs_content::pilots` measures a mission's zeppelin *carrier* surface
  (`survey_retail_zeppelin_carrier`, `team_spellings`), not a pilot identity
  list; the F33-A pilot roster is declared design and the F33-D retail
  measurement has not run.
- F30-A declares the directed-relation *schema* (`DeclaredRelation`); no
  measurement binds the installation's own factions into a relation table, and a
  table over the fixture's factions would be a fabricated faction table.

These are the same gaps the F14-D.6 finding records for the `ammo`,
`hardpoint_equipment` and `gun` collections. They are resolved by the follow-up
tasks created from this task and listed in the evidence report's
`review.method`.

## The airframe display-name mapping

`cs_app::mission_start::airframe_entry` maps a scenario's `player_plane` /
`enemy_plane` display name onto the airframe table row that names the scene root
`support\planes.gw` creates (`set planeOutput player_<x>`). The F14-D.6 airframe
collection keys its rows by exactly that root (`airframe/<root>`), so the option
table's airframe list and the mapping must be the same eleven airframes. The
acceptance test in `cs_app` cross-checks that they are: for every row of
`AIRFRAME_TABLE`, `airframe_entry(display_name)` resolves to a scene root whose
`airframe/<root>` id the option table offers, and the union covers every row and
nothing else. (`cs_app::mission_start` is outside this task's owner paths, so it
is called, never edited.)

## Design decisions

- **A table before validation, not a silent one.** Splitting the assembled
  `ScenarioOptionTable` from the validated `ScenarioOptions` is what lets an
  only-partly-measured installation be honest: the table says which axes are
  missing (`empty_required_lists`, `unmeasured_dimensions`) and `validate`
  refuses them by name, instead of omitting a dimension or filling it from the
  fixture.
- **One evidence record per dimension, required.** `from_lists` refuses a table
  that does not say how one of its lists was established
  (`MissingOptionEvidence`), so a caller can never leave a dimension's
  provenance unstated.
- **The closed vocabulary is a Rust enum.** A difficulty tier or victory
  condition "outside the vocabulary" is not representable; what is checkable is
  that the retail difficulty list is exactly the three measured steps and that
  the designed extension tier (`Elite`) stays out. `UnknownDifficultyTier` and
  `UnknownVictoryCondition` are therefore unreachable defence-in-depth checks
  left as they were by F49-A; this stage does not add a way to construct an
  out-of-vocabulary value.
- **The installed read is shared.** `retail_scenario_options_from_baseline`
  assembles over an already-read baseline, so the acceptance test that
  re-derives each measured list reads the installation once rather than twice.

## Unknowns met (recorded, not guessed)

- The **loadout, pilot and relation** option lists have no production producer
  in this workspace (above). Affected content: the custom scenario's ordnance,
  crew and faction-relation options; the `Non-negotiable 2` "unsupported
  ordnance" refusal cannot name a supported set until F27 produces loadout
  identities. Resolving tasks: filed from this task.
- The original's **player-count limit** is unmeasured (F49-A finding). The bound
  here is the module's own design bound; a real measurement would replace it.
- The original's **victory vocabulary** is unmeasured; F49-A authors it and this
  stage preserves it as declared design.
- The **environment** ids are per-mission weather environments, not a measured
  installation-wide environment table; whether the original's IA screen offers
  each of them is F49-D.03/F19 territory.
- **Relations between the installation's own factions** are unmeasured; the
  option table declares none rather than a fixture set.

No retail data was written or committed; the retail tests read the installation
read-only. The per-dimension sources are the earlier evidence stages' own
claims (F14-D.3, F14-D.5, F14-D.6, F27-D, F32-D, F19/F20), named in the
`OptionListEvidence::Measured` reader strings and in `docs/findings/evidence/
F49-D.02.json`.
