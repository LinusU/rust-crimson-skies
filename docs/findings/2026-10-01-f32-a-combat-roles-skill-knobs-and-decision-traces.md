# F32-A: Combat roles, skill knobs and decision traces

Date: 2026-10-01. Task: F32-A "Define combat roles, skill knobs and
decision traces" (`specs/F32-ai-combat-formations-aces-and-difficulty.md`,
section `### F32-A`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only (no `CS_GAME_DIR` read, no evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/ai.rs` (new): the declared, provenance-carrying
  half — `DeclaredCombatRules` (subject, `Origin`, role definitions, ace
  variants, priority policy, formations, difficulty profiles,
  `Provenance`), `DeclaredCombatRole`, `RoleArsenal`, `SkillKnobs`,
  `PriorityPolicy`, `SkillKnob`/`SkillKnobValue`/`SkillKnobOverride`,
  `DeclaredAceProfile`, `DifficultyTier`/`DifficultyProfile`,
  `FormationId`/`DeclaredFormation`/`FormationRecovery`/`RecoveryPolicy`,
  the `CombatSchemaError` validation and the minimal synthetic fixture
  (`declared_synthetic_combat_rules`, `declared_synthetic_ace_profile`,
  `declared_synthetic_difficulty_profile`).
- `crates/cs_sim/src/ai/combat.rs` (new): the runtime contract —
  `CombatPlanner`, `SkillProfile` (knobs + priority policy), the
  `CombatRole` mirror, `RoleAssignment`, `CandidateView`/`ThreatEvidence`,
  `ArsenalSnapshot`/`MountAvailability`, the `FormationId`/
  `FormationFacts`/`RecoveryTrigger`/`RecoveryAction` vocabulary,
  `CombatRequest` → `CombatDecision` with its `DecisionTrace`
  (`CandidateTrace`, `TermScore`, `RejectReason`, `FireVeto`,
  `HoldReason`), `CombatError`, and the synthetic fixture
  (`synthetic_combat_planner`, `synthetic_candidate`, `synthetic_threat`,
  ...).
- `crates/cs_sim/src/ai/mod.rs`, `crates/cs_content/src/lib.rs`
  (wiring only): module declaration and the crate-level doc paragraph.
- `crates/cs_sim/tests/ai/main.rs` plus
  `crates/cs_sim/tests/ai/accept_f32_a_combat.rs` and
  `crates/cs_sim/tests/ai/accept_f32_a_combat_failures.rs`: the
  `accept_f32_a_*` runtime acceptance tests. The path `crates/cs_sim/tests/ai/`
  is this task's owner path, so the runtime half is tested from
  `tests/ai/main.rs` (Cargo auto-discovers a `tests/<dir>/main.rs` target).
- This file.

**Why the declared-schema tests are unit tests, not `cs_content/tests/`:**
`crates/cs_content/tests/` is *not* an owner path of this task, and a
`cs_sim` test cannot reach `cs_content` (`docs/01-ARCHITECTURE.md`: `cs_sim`
may depend only on `cs_types` and `cs_script`). The declared half is
therefore tested by `#[cfg(test)]` tests inside
`crates/cs_content/src/ai.rs`, which is production code in an owner path;
they are named `accept_f32_a_*` like every other acceptance test in the
repository and are selected by the same `cargo test --workspace --locked --
accept_f32_a_` filter.

**One observable failure:** with the declared priority policy, an escort
whose protected actor is under an *authoritative* attack must select the
attacker, not the nearest hostile. The fixture puts the attacker 800 m away
and a harmless hostile 300 m away, so a policy that scores only proximity —
or that scores "threatening my charge" without demanding the authoritative
`HitEventId` evidence — picks the 300 m raider.
`accept_f32_a_escort_prioritizes_attacker_threatening_protected_actor`
asserts the attacker wins, and its failure cases assert the flip: the same
fixture with the protected-actor weight removed, with the evidence pointing
at a different victim, and with the evidence younger than the profile's
reaction delay each select the 300 m raider instead. Every test calls
production code (`CombatPlanner::decide`, the declared-schema validator and
fixture), so removing a layer fails to compile.

## Semantics defined at this stage

- **Role vocabulary.** `DeclaredCombatRole` is the seven behaviors the sheet
  names — fighter attack, bomber run, torpedo run, escort, interception,
  evasion and retreat — with a `RoleArsenal` requirement per role. Whether
  the original game uses exactly this role set, and which weapon each role
  needs, is designed vocabulary (F32-D's retail stage).
- **Priority policy.** `PriorityPolicy` is four weighted terms
  (`ProtectedActorThreat`, `ScriptObjective`, `SelfDefense`, `Proximity`)
  plus the `threat_window_ticks` an authoritative attack stays live for.
  Every value is a `Resolved<T>`: a value content could not evidence stays
  an explicit unknown and refuses to lower, so no session runs combat AI
  under a guessed weight or window.
- **Hostility is a gate; friendly fire and line of fire are separate
  predicates** (non-negotiable 3). A candidate must be a *declared* hostile
  to be selectable at all; `CandidateTrace::fire_veto` reports the
  line-of-fire veto on a *selected* hostile independently, so "this is my
  target" and "I may shoot past this friendly" never collapse into one
  answer.
- **Threats come from authoritative events only.** `ThreatEvidence` carries
  the damage system's `HitEventId`; a candidate's "attacks my protected
  actor" term is scored only when that evidence names the protected actor
  inside the declared window, never from proximity, faction or role
  (non-negotiable 3/4, mirroring F30's threat ledger).
- **Skill knobs are a closed behavior vocabulary.** `SkillKnob` has exactly
  eight variants — reaction delay, aim error, engagement range, fire
  discipline and the four priority weights — each with a unit. There is
  deliberately no damage, armor or health variant, so an ace variant is a
  list of *behavior* overrides and cannot be inflated health
  (F32 "Deliverable and interfaces"). A unit or range mismatch in an
  override is refused.
- **Difficulty moves those knobs and nothing else.** `DifficultyProfile` is
  a named `DifficultyTier` plus `SkillKnobOverride`s, so the type cannot
  express a simulation rate, a tick rate or a damage multiplier:
  non-negotiable 1 ("never increase simulation speed to fake difficulty")
  is a property of the vocabulary, checked by
  `accept_f32_a_difficulty_profile_moves_only_evidence_backed_knobs`.
- **Recovery paths are declared, not invented per tick.**
  `DeclaredFormation` names its leader, members and one `RecoveryPolicy`
  per recovery trigger (leader loss, assigned-target destruction, route
  interruption — non-negotiable 4). The runtime reports which trigger is
  pending and the policy the formation declared; the stateful follower
  recovery itself is F32-C (its AC03 scenario).
- **Determinism.** `CombatPlanner::decide` is a pure function of the
  planner's immutable policy and one typed request. The selection order is
  the total `(score desc, distance asc, ActorId asc)`, so the candidate
  order the ECS presented cannot change a decision.
- **No omniscience.** The planner sees only what the request hands it:
  the candidates the approved perception model reported, the allegiance
  `cs_sim::targeting` resolved, the objective flag mission rules set, and
  the authoritative threat evidence. It never queries the world itself.

## Unknowns recorded (not guessed)

- The original game's AI role set, the weights it uses to pick a target,
  the reaction times, aim error, engagement ranges, formation membership
  and recovery behavior, and its difficulty option's effect on any of them
  are **unmeasured**. F13 locates mission programs but recovers no AI
  tuning; F32-D is the retail stage. Every constant, weight, role and
  fixture here is newly authored project design with designed provenance.
- Whether the original escort role is a hard behavior class or a
  script-assigned objective, and whether the original difficulty setting
  changes AI parameters at all (as opposed to damage multipliers or player
  aids), are unknown and recorded, not assumed.
- The declared→runtime lowering boundary (`cs_content::ai` →
  `cs_sim::ai::combat`, refusing every `Resolved::Unknown`) belongs to
  `cs_app::ai::combat`: role/priority lowering is F32-B, ace/difficulty/
  formation lowering is F32-C. This task does not touch `crates/cs_app`
  (not an owner path).
- `ArsenalSnapshot` is the typed *input* an ace's firing solution reads
  (F32-B's AC02). F32-A defines the snapshot and the requirement that a role
  may only use a mount the snapshot reports usable; it does not implement
  the firing solution.

## Not claimed

No original-data verification, no ECS wiring, no maneuver selection, no
firing solution, no stateful formation recovery, no spawn ownership
(non-negotiable 5 stays with mission execution) — F32-B/C/D own those. The
task awards at most **checked** status.
