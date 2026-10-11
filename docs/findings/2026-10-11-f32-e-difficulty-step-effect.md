# F32-E: what a difficulty step changes — searched, not found; tier knobs bound to declared alternatives

Date: 2026-10-11. Task: #566 `F32-DIFFICULTY-EFFECT`. Follows
`2026-10-03-f32-d-original-ai-roles-and-difficulty.md`. Capability used:
`retail` (read-only), plus ordinary build/test. No original run, no GPU, no
audio, no human play.

## Search result (the negative is the result)

* F32-D already measured, over all 62 reader archives, that none of the eight
  instant-action scenario descriptors carries a difficulty key at any depth,
  and that the option is a three-step selector (ids `109..=111`) whose only
  consumers found are the options-screen title/description strings and one
  instant-action label.
* This task re-scanned the loose files of `$CS_GAME_DIR` (text search for
  `difficult` and for the resource macros `IDS_DIFFICULTY` /
  `IDS_GO_DIFFICULTY`): the only loose hit is the EULA. Archive members are
  compressed, so that scan says nothing about them; for archives the F32-D
  census is the evidence.
* **Not searched, so unknown:** the executable image. A numeric table keyed by
  the option value would live in code, and finding it needs the disassembly
  work no agent task is authorised to commit. Whether a step changes AI
  behaviour, damage, spawn counts or anything else is therefore **unknown**,
  not "nothing".

No original display text, step name or value was copied.

## What changed (declared rule instead of a measurement)

`cs_content::ai`:

* `SkillKnobOverride` gains `alternative_of: Option<SkillKnob>`;
  `try_new` leaves it `None`, `try_new_alternative` sets it to the knob moved.
* `DifficultyProfile::try_new` refuses, with the new
  `CombatSchemaError::UnevidencedTierOverride { tier, knob }`, any override that
  is neither `VerifiedOriginal` with a source span nor `Designed` with
  `alternative_of == Some(its own knob)`. Inferred, unknown, documented and
  contradicted provenance are refused too. Ace overrides are unaffected.
* A tier still cannot move a rate, damage multiplier or tick rate: the
  `SkillKnob` vocabulary is closed and has none (tests pin this).
* `cs_sim::ai::combat` needed no change: its per-tier `SkillProfile`s are
  lowered runtime values, never built from `DifficultyProfile`.

Tests: `accept_f32_e_*` in `crates/cs_content/src/ai.rs`.

## Unknown (still open)

| claim id | unknown | resolving task |
| --- | --- | --- |
| `f32.d.limit.difficulty_effects` | What a step changes; executable image unsearched. Every tier knob remains a *designed alternative*. | needs executable analysis by the owner; #566 closes the declared-alternative half |
