# F32-S: what a per-aircraft skill tier does — not measured; declared as vocabulary

Date: 2026-10-11. Task: #567 `F32-SKILL-EFFECT`. Follows
`2026-10-03-f32-d-original-ai-roles-and-difficulty.md` and
`2026-10-11-f32-e-difficulty-step-effect.md`. Capability: ordinary build/test
only; no new retail measurement, no original run.

## Measured (F32-D, unchanged)

Forty labels (thirty-two enemy groups, eight named aces) drawn from exactly
`novice` 8, `veteran` 13, `ace` 19, in eight instant-action scenario
descriptors.

## Not measured

* What a tier changes. No reachable scenario data binds a tier to a reaction
  time, aim error, fire discipline, loadout or anything else, and the
  executable image, where such a table would live, is unsearched. Whether the
  tier is cosmetic (e.g. only selects a loadout) is likewise **unknown**, not
  "nothing".
* Any relation to the difficulty option. Both are three-valued; nothing
  measured maps one onto the other, and nothing here does.

## What changed

`cs_content::ai`:

* `DeclaredSkillAssignment` names the tier of `SkillTarget::Group(n)` or
  `SkillTarget::ScenarioAce`. It is built from the importer's label via
  `DeclaredSkillTier::from_label`; `from_group` / `from_ace` take the
  `stunts` records, so a bare `skill()` string no longer has to be carried.
  An unrecognised label is refused as `CombatSchemaError::UnknownSkillTier`
  (target and label verbatim).
* `DeclaredCombatRules::with_skill_assignments` stores them, refusing a target
  assigned twice (`DuplicateSkillTarget`); `skill_tier_of` reads one back.
* Declared→runtime: `DeclaredSkillTier::knob_overrides` always returns
  `SkillTierEffectUnmeasured`. It is refused with a reason, not dropped and not
  filled with a guessed table. `ORIGINAL_SKILL_TIER_EFFECT_MEASURED = false`.
  A mission that wants a tier to behave differently must declare its own
  knobs or ace profile as a designed alternative.
* `cs_sim::ai::combat` is unchanged; its `SkillProfile`s are never derived from
  a tier.

Tests: `accept_f32_s_*` in `crates/cs_content/src/ai.rs`.

## Unknown (still open)

| claim id | unknown | resolving task |
| --- | --- | --- |
| `f32.d.limit.skill_tier_effects` | What novice/veteran/ace change at runtime; executable image unsearched. | needs executable analysis by the owner; #567 closes the declared-vocabulary and refusal half |
