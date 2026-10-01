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
  `declared_synthetic_formation`, `declared_synthetic_difficulty_profiles`).
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

Test counts: 32 `accept_f32_a_*` tests in `crates/cs_sim/tests/ai/` (17 in
`accept_f32_a_combat.rs`, 15 in `accept_f32_a_combat_failures.rs`) and 7
`accept_f32_a_*` unit tests in `crates/cs_content/src/ai.rs` — 39 selected
by `cargo test --workspace --locked -- accept_f32_a_ --include-ignored`.

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
  nine variants — reaction delay, aim error, engagement range, fire
  discipline, threat window and the four priority weights — each with a
  unit. There is deliberately no damage, armor or health variant, so an ace
  variant is a list of *behavior* overrides and cannot be inflated health
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
  interruption, protected-actor loss — non-negotiable 4). The runtime
  `FormationFacts::pending_trigger` names the single pending trigger in a
  fixed order and `CombatPlanner::decide` reports it with the action the
  formation declared; a trigger with no registered `RecoveryPolicySet` is
  refused rather than dropped. The stateful follower recovery itself is
  F32-C (its AC03 scenario).
- **An ace is a variant of a role, not a second role.**
  `CombatRequest::profile` takes the actor's effective profile: `None` runs
  the planner's profile for the assigned role, and `Some` runs a declared
  variant (an ace, or a difficulty-overridden copy) whose role must match
  the assignment — `CombatError::ProfileRoleMismatch` refuses a fighter
  profile handed to an escort. This keeps "aces are data-driven
  behavior/skill variants" a property of the type: the whole runtime
  vocabulary has no damage, armor, health, tick-rate or time-scale field
  anywhere.
- **The reaction gate is part of the trace, not a silent drop.** A threat
  younger than the profile's `reaction_ticks` scores zero on its terms and
  is recorded as `ReactionState::Deferred { age_ticks, required_ticks }`,
  so a profile's skill difference is inspectable rather than invisible.
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

## Sensitivity probes (run and reverted; no probe committed)

Each probe removes one load-bearing rule and names the test that caught
it. All were reverted; the committed tree is the green one.

1. `PriorityTerm::ProtectedActorThreat => 0.0` (the protected-actor threat
   term removed from the scoring) → 4 failures:
   `accept_f32_a_escort_prioritizes_attacker_threatening_protected_actor`,
   `accept_f32_a_line_of_fire_veto_is_reported_on_the_selected_hostile`,
   `accept_f32_a_ace_variant_reacts_where_a_slow_profile_has_not_noticed_yet`,
   `accept_f32_a_decision_is_independent_of_candidate_order`.
2. Scoring a threat only when the evidence's victim is the *observer*
   (self-defense evidence used for the protected-actor term) → the same 4
   failures: the AC01 scenario's attack on the charge would score nothing.
3. `trace.reaction = if false && age < reaction_ticks` (the reaction gate
   removed) → `accept_f32_a_ace_variant_reacts_where_a_slow_profile_has_not_noticed_yet`
   fails: the slow escort would notice a 10-tick-old attack.
4. `traces.sort_by(compare_traces)` removed (the trace follows the request
   order) → 5 failures, including
   `accept_f32_a_decision_is_independent_of_candidate_order`, which is the
   test that exists for exactly that property.
5. `if false && value.unit() != knob.unit()` in the declared schema's
   validator (a distance accepted in a weight slot) →
   `accept_f32_a_difficulty_profile_moves_only_evidence_backed_knobs`.
6. `if false && id.kind() != ContentKind::Pilot` in `DeclaredAceProfile::try_new`
   → `accept_f32_a_ace_variant_is_a_behavior_override_of_its_base_role`.
7. `if false && !seen.insert(change.knob)` in the declared duplicate-knob
   check (the last override would silently win) →
   `accept_f32_a_difficulty_profile_moves_only_evidence_backed_knobs`.

## Commands run (all exit 0)

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --locked` (1812 tests passing)
- `cargo test --workspace --locked -- accept_f32_a_ --include-ignored`
  (36 tests: 29 in `cs_sim`'s `ai` target, 7 in `cs_content`'s lib)

## Review (bunny-2, same session as the implementation — not independent)

The same agent that wrote this stage also reviewed it, so this section is a
self-review, **not** independent evidence and not a substitute for the
owner's review. What it did: re-derived the AC01 numbers by hand from
`decide`, re-ran every probe below, and fixed what it found.

### Fixed

1. **The protected actor's session generation was only checked when the
   caller also reported its lifecycle** (`if let (Some(actor), Some(_)) = …`).
   A request with `protected_alive: None` therefore carried a stale
   generation's actor into the decision unchecked. The identity check is now
   unconditional, and `accept_f32_a_foreign_protected_actor_is_refused_without_a_lifecycle_report`
   covers all three lifecycle values.
2. **The formation's `assigned_target` identity was never checked** while
   its `leader` was. It is now refused by the same rule, and
   `accept_f32_a_foreign_formation_leader_is_refused` covers both.
3. **`FormationFacts` and `RoleAssignment` could disagree about which
   formation the observer is in**, and the recovery path was then resolved
   from the facts' formation. `decide` now refuses
   `FormationAssignmentMismatch`. This tightened one existing fixture:
   `accept_f32_a_foreign_formation_leader_is_refused` had been passing
   formation facts for an assignment that declared no formation at all,
   which is the contradiction the new refusal names.
4. **Two tests asserted nothing about production code.**
   `accept_f32_a_observer_and_destroyed_actors_are_never_targets` built a
   `Vec` of the same actor once per `LifecycleKind` and compared its length
   — it exercised no code at all. It is split into the observer-itself case
   and `accept_f32_a_a_real_damage_event_destroys_the_charge_and_its_id_is_the_threat_evidence`,
   which resolves a real lethal `HitEvent` through a real
   `cs_sim::damage::DamageResolver`, asserts the charge's
   `LifecycleKind::Destroyed` and then hands the planner the very
   `HitEventId` the resolver stamped. That also upgrades the
   "authoritative evidence" claim from a hand-built id to a producer-stamped
   one. The arsenal test's closing `assert!(matches!(
   FireVeto::ArsenalUnusable { .. }, FireVeto::ArsenalUnusable { .. }))`
   matched a constructed value against its own variant and observed
   nothing; it now runs a decision with a fully disarmed arsenal and asserts
   the veto on the target, which is the first real coverage of that variant.
5. **The declared schema accepted an engagement range of 0 m that the
   lowered runtime profile refuses** (`cs_sim::ai::combat` bounds the range
   below by `PROXIMITY_EPSILON_M` so the proximity term stays normalizable).
   `MIN_ENGAGEMENT_RANGE_M` now refuses it at declaration too, so a record
   the lowering boundary would reject cannot validate here.
6. Documentation: the proximity term is zeroed for a refused candidate while
   the other three terms are still reported, which was true but unstated;
   `CandidateTrace` scoring now says so. `CombatPlanner::decide`'s contract
   now states that it is target *selection* only — what an `Evade` or
   `Retreat` assignment does about the target it is given is F32-B's.

### Review commands run (all exit 0)

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --locked` — 1815 passed on the review branch
  before the rebase, 1823 after rebasing onto the current `origin/main`
  (the 8 extra tests come from `main`, not from this branch)
- `cargo test --workspace --locked -- accept_f32_a_ --include-ignored` —
  39 tests (32 in `cs_sim`'s `ai` target, 7 in `cs_content`'s lib), all
  passing

### Review sensitivity probes (run and reverted; none committed)

1. Formation/assignment mismatch check disabled → 1 failure
   (`…formation_facts_that_contradict_the_assignment_are_refused`).
2. Protected-actor session check gated on the lifecycle report again → 1
   failure (`…foreign_protected_actor_is_refused_without_a_lifecycle_report`).
3. `assigned_target` dropped from the session check → 1 failure
   (`…foreign_formation_leader_is_refused`).
4. `ArsenalUnusable` veto branch disabled → 1 failure
   (`…arsenal_snapshot_reports_separate_availability_counts`).
5. `MIN_ENGAGEMENT_RANGE_M` back to `0.0` → 1 failure
   (`accept_f32_a_difficulty_profile_moves_only_evidence_backed_knobs`).
6. `ProtectedActorThreat` term zeroed (re-verification of the implementer's
   probe 1) → 6 failures, including the AC01 scenario.
7. Observer-itself gate disabled → 1 failure
   (`…observer_is_never_its_own_target`).

## Noted for the owner, not fixed here

- `crates/cs_content/tests/` is **not** an owner path of this task, so the
  declared half's acceptance tests are `#[cfg(test)]` tests inside
  `crates/cs_content/src/ai.rs` instead of the
  `crates/cs_content/tests/accept_f*.rs` files every predecessor stage
  (F24-A, F27-A, F28-A, F29-A, F30-A, F31-A) used. The tests are real and
  selected by the same filter, but the asymmetry is caused by the task's
  owner paths, not by a technical limit: a `cs_content/tests/accept_f32_a_*.rs`
  would work fine. Future "define the X schema" tasks should name that path.
- F32-A implements a *policy function* (`CombatPlanner::decide`) rather than
  only type declarations. This is required by the stage's own minimum
  scenario (AC01 needs something that ranks targets) and bounded by
  F32-B's AC02 (the firing solution is not implemented — only its typed
  input) and F32-C's AC03 (the stateful recovery is not implemented — only
  its declared-path reporting). A reviewer should confirm they agree that
  this split respects `docs/TASK-SPLITTING.md`.
