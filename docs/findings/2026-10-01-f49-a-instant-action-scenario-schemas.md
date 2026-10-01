# F49-A: Instant Action preset and custom-scenario schemas

Task #189 (`specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
`### F49-A`). Declared schema in `cs_content::instant_action`; selection and
lowering boundary in `cs_app::ui::instant_action`; tests are the
`accept_f49_a_*` unit tests in `crates/cs_content/src/instant_action.rs` and the
integration tests in `crates/cs_app/tests/accept_f49_a_preset_spawn.rs`.

All behavior is **designed**, synthetic and not original-verified. Awards at
most *checked*. No original preset, option table, screen or file was read.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/instant_action.rs` (new): the declared schema —
  `InstantActionCatalog`, `InstantActionPreset`, `ScenarioParameters`,
  `ScenarioRoster`/`ScenarioActorSpec`, `VictoryRules`/`VictoryCondition`/
  `RespawnBudget`/`TieOutcome`, `ScenarioSeed`, `ScenarioOptions`,
  `CustomScenarioDraft`/`CustomScenarioRequest`, `ScenarioProblems` and the
  synthetic fixture catalog.
- `crates/cs_app/src/ui/instant_action/mod.rs` (new): `ScenarioSelection`,
  `LoweredScenario`, `lower_preset`, `lower_custom`, `resolve_custom`,
  `preset_rows`, `custom_dimensions`, `wingmate_slot`, `LowerError`.
- `crates/cs_content/src/lib.rs`, `crates/cs_app/src/lib.rs`: wiring only
  (module declaration and the crate-level doc entry).
- This file.

**One observable failure:** select a preset whose world is unmeasured, or whose
enemy roster carries an unknown plane: before this work nothing resolved a
preset selection at all, and no `accept_f49_a_*` test existed. Removing the
roster lowering makes
`accept_f49_a_every_discovered_preset_launches_with_its_expected_actors_world_and_rules`
fail (no actors); substituting a default plane for the unknown makes
`accept_f49_a_an_unmeasured_plane_refuses_rather_than_defaulting` fail; ignoring
the declared rules makes the same AC01 test fail on the condition, deadline and
respawn budget.

## Designed semantics

- **A preset is a scenario, not campaign with rewards off.** Each preset keeps
  two identities: the `ia_preset` id a player selects and the `ia_scenario` id
  it launches. The catalog refuses a duplicate of either, and two presets may
  never launch one scenario — otherwise "which authored scenario did I launch"
  would be unanswerable, which is what non-negotiable 1 asks to preserve.
- **One parameter set for preset and custom.** `ScenarioParameters` is shared,
  so a custom scenario is describable by the same vocabulary as a preset. If the
  two had separate shapes, "custom" would quietly be a lesser mission type.
- **A side is not a faction.** `ScenarioSide` (`Player`/`Ally`/`Enemy`/
  `Neutral`) is the spawn side; a faction is a `ContentId` and allegiance is a
  directed relation from `cs_content::target_rules`. Relabeling a side can never
  relabel a faction, and validation refuses a roster whose declared relations
  make it impossible to fight (same faction on both sides, or a friendly/neutral
  declared relation between the player's and the enemy's faction).
- **All problems, once.** `validate_custom` returns a sorted `ScenarioProblems`
  list rather than the first failure, and each problem names its dimension, the
  selection to replace (or the open claim for an unknown) and a detail. AC04's
  "actionable validation errors" is this list; `require_valid_custom` is the
  gate, and the boundary validates **before** constructing any runtime identity,
  so a refused selection never half-spawns.
- **Selection is content ids only.** `ScenarioSelection` has no row index and no
  field that could hold cash, ownership or an objective, which is how F49
  non-negotiable 3 holds by construction at this stage: an Instant Action run
  cannot write campaign progression because this type has nowhere to write it.
- **Unknowns refuse.** Every scenario value is `Resolved`. An unknown plane,
  loadout, survivability or world lowers to `LowerError::UnknownValue` carrying
  the original claim id and reason verbatim; a preset's unmeasured display name
  stays `Resolved::Unknown` and the selectable row shows `title: None` rather
  than an invented name. Unknown *relations* produce no roster problem (they
  cannot prove impossibility) and refuse at the lowering boundary instead of
  being guessed into a faction table.
- **Seeds are explicit.** `ScenarioSeed` is a required field of both a preset
  and a custom request; no path draws an implicit seed. Streams are derived
  through `SplitMix64::for_domain` with this module's own
  `SCENARIO_SEED_DOMAIN`, so adding a consumer later never shifts a value
  another consumer already observed.
- **Roster ordering is canonical.** `ScenarioRoster::try_new` sorts by
  `(side, slot)`, so a given choice set has exactly one lowering. Reversing the
  input lowers to the same plan — which is the baseline AC02's one-slot change
  will be measured against in F49-B.
- **A visible option is a real option.** `custom_dimensions` derives every
  selectable group from `ScenarioOptions`, so a dimension the catalog does not
  declare cannot be displayed; and every offered airframe and loadout is proven
  selectable by lowering it, while an unlisted one is refused by the same path.

## Reused rather than redefined

`DeclaredSurvivability` (F33-A), `DifficultyProfile`/`DifficultyTier` (F32-A),
`DeclaredRelation` (F30-A), `WorldId` (F18-A), `EnvironmentId` (F19-A),
`ContentId`/`Origin`/`Provenance`/`Resolved` (F14-A), `FactionId`/`GeometryId`/
`PilotId`/`SurvivabilityPolicy`/`WingmateSlot` (cs_sim, F33-A) and
`SplitMix64` (F00-SEED).

## Unknowns met (recorded, not guessed)

None of these is answered by this stage; each is F49-D's or an earlier
evidence stage's work.

- **The original Instant Action preset list and its count.** The F31-D retail
  route-coverage audit observed **8** `IA#` mission directories in the
  installation (`docs/findings/2026-10-01-f31-d-route-coverage-in-every-mission-type.md`,
  "8 Instant Action scenarios are covered"), but that is a count of *scenario
  directories carrying a route carrier*, not a measured preset table. Whether
  those eight are player-selectable presets, how many presets a player sees, and
  whether presets and scenarios are one-to-one are **unmeasured**.
- **Each preset's parameters**: world, environment, roster sizes and
  composition, difficulty, victory rule, respawn budget, tie rule, seat count and
  seed. Nothing in this stage claims any of them; the fixture presets are
  authored to be *distinguishable*, not to resemble the original.
- **The original preset display names.** Unmeasured, so `title` stays unknown
  rather than being written from a directory name.
- **The supported custom-scenario dimensions and their per-world contents**:
  which worlds and environments a player may pick, which planes and loadouts,
  which factions, how many difficulty steps, which victory conditions, the player
  count limits and the tie vocabulary. `ScenarioOptions` is a declared list; its
  values are the fixture's.
- **Whether Instant Action is single-player in the original**, and whether any
  preset offers more than one human seat. The fixture presets seat one player by
  construction; `MAX_SCENARIO_PLAYERS` is a bound on this module's own request
  type, not a measured original limit.
- **Whether an ally actor maps one-to-one onto a runtime wingmate slot.** The
  boundary reports the mapping through `wingmate_slot` for slot 0 of the ally
  side; what the original does with more than one ally is F49-B's spawn work.
- **Whether the original IA rosters use per-slot pilot identities**, and what a
  preset's neutral traffic is. The fixture assigns one synthetic pilot to every
  actor; a real pilot assignment is F33-A/F49-D territory.
- **The original seed policy**: whether the original seeds a scenario at all,
  from where, and whether a replay records it. This stage requires an explicit
  seed on every scenario because F49 non-negotiable 4 demands no hidden
  randomization; that is a project rule, not a measurement of the original.

No retail data was read for this task: F49-A's required capability is ordinary
build/test only, and every value here is synthetic.