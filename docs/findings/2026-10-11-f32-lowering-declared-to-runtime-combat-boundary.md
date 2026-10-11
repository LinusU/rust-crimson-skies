# F32-LOWERING (#551): the declared-to-runtime AI combat boundary

`cs_app::ai::combat` lowers `cs_content::ai::DeclaredCombatRules` onto
`cs_sim::ai::combat`: role profiles (`lower_profile`, `lower_profiles`), the
difficulty roster (`lower_difficulty`), ace variants (`lower_ace`), formation
rosters and recovery paths (`lower_formation_roster`,
`lower_recovery_policies`), role assignments (`lower_assignment`) and a whole
`CombatRuntime` (`lower_runtime`).

## Rules the boundary enforces

- Every `Resolved::Unknown` knob or weight refuses with
  `LoweringError::UnknownValue` naming the record, field and claim.
- A role with no declared profile, a tier with no declared difficulty profile,
  an unbound formation slot, and an ace assigned to a role other than its
  base role each refuse by name.
- A difficulty tier is composed onto the role profile here, once; an ace's
  overrides apply on top of that tier's profile. `cs_sim` never re-applies an
  override list.

## Unknowns and limits recorded

- The original's roles, knobs, tiers, aces and formations remain unmeasured
  (F32-D). Everything lowered from the synthetic fixtures is designed data and
  is not original-reference evidence.
- An ace variant is lowered for one tier (`lower_runtime`'s `ace_tier`),
  because the runtime keys a variant to a tier. Which tier an original ace
  belongs to is unknown.
- Mission-side binding of slots to live actors, and the ace/tier choice per
  actor, is the caller's; no running mission consumes the lowering yet.

Tests: `crates/cs_app/tests/accept_f32_e_combat_lowering.rs`.
