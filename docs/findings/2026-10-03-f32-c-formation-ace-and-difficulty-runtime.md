# F32-C: Formations, aces and difficulty profiles in the session runtime

Date: 2026-10-03. Task: F32-C "Integrate formations, aces and difficulty
profiles" (`specs/F32-ai-combat-formations-aces-and-difficulty.md`, section
`### F32-C`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence
report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/ai/combat.rs` (owner path): the F32-C runtime half —
  `DifficultyTier`, `DifficultyRoster`, `AceId`, `AceVariant`,
  `FormationRoster`/`FormationRosterMember`, `FormationMemberReport`,
  `FormationTick`, `StationAnchor`/`StationAssignment`/`station_offset_m`,
  `FormationUpdate`, `FormationCoordinator`, `ProfileSource`,
  `CombatantRequest`, `CombatStep` and `CombatRuntime`, plus the new
  `CombatError` variants (twenty-one in total after review) and the F32-C
  synthetic fixtures. One additive change
  outside the new code: `RecoveryTrigger` gained `PartialOrd, Ord`.
- `crates/cs_sim/tests/ai/accept_f32_c_combat.rs` (new, owner path): the twelve
  `accept_f32_c_*` scenario and integration tests (fourteen after review).
- `crates/cs_sim/tests/ai/accept_f32_c_combat_failures.rs` (new, owner path):
  the nine `accept_f32_c_*` refusal and teardown tests (ten after review).
- `crates/cs_sim/tests/ai/main.rs` (owner path, wiring only): declares the two
  new test modules and updates the target's doc comment.
- This file.

**One observable failure:** the minimum scenario. Destroy formation 1's leader
on the tick it is destroyed. A runtime that only *reports* a recovery — F32-A's
`RecoveryOutcome` on its own — leaves the followers anchored on a destroyed
actor or on nothing at all, so they either fly to a wreck forever (a permanent
orbit) or receive a station produced by dividing by a degenerate vector (a
NaN). `accept_f32_c_leader_destroyed_mid_turn_its_followers_recover` asserts
that the leader is actually replaced, that no station or anchor ever names the
destroyed actor again, that every station is a finite point equal to its
anchor plus the slot's authored offset, and that forty further unchanged ticks
produce a bit-identical station.

## Semantics defined at this stage

- **Recovery is applied, not only reported.** F32-A's
  `RecoveryOutcome` *reports* which declared path applies;
  `FormationCoordinator::apply` is the half that *acts*. It reconciles one
  tick's report, raises the pending trigger in the same fixed precedence
  `FormationFacts::pending_trigger` uses, applies the declared action, and
  returns the resulting leadership and stations as a `FormationUpdate`.
- **The coordinator produces the facts the planner consumes.**
  `CombatRuntime::step` names a `FormationId`; the runtime builds the
  `FormationFacts` from its own coordinator, so a promoted leader is the leader
  the next decision reasons about and a decision can never be made against
  facts the coordinator does not believe. The recovery the trace reports and
  the recovery the coordinator applies therefore cannot disagree: both read
  `FormationFacts::pending_trigger` and `RecoveryPolicySet::action`.
- **A station is a finite position anchored on a living member.** A station is
  a world point plus a radius, never a heading, an attitude or a control law,
  so following it cannot make this module a second pose owner
  (`docs/contracts/FLIGHT-PHYSICS.md`); navigation consumes it. The offset is
  `station_offset_m(slot)`, an authored constant per slot index — deliberately
  not a normalized direction, because `normalize(zero_vector)` is the classic
  way to put a NaN into a guidance input. The anchor is either the *living*
  leader or the survivors' centroid; `StationAnchor::Leader` is never built
  for a destroyed member.
- **A recovery is answered once per fact, not once per trigger.** A latched
  trigger stays reported in `FormationUpdate::latched` and is not answered
  again until its fact recovers *or is replaced*: without the latch a declared
  path would be re-applied every tick, which is a permanent state, not a
  recovery. A latch that outlived its fact is the same permanent state with the
  opposite cause — a promoted leader that is then destroyed in turn is a second
  loss, not a repeat of the first, and an assigned target that is no longer the
  assigned target is a new loss even if the new one is dead too. Both are
  released when they are replaced. (The first version of this change got this
  wrong; see the review section.)
- **Teardown is a real state, not a flag.** A formation whose members are all
  destroyed, or whose declared path is `Withdraw`, is removed from the
  coordinator: `facts` returns `None` and a later report is refused as
  `UnknownFormation`, so no membership, leadership or station of the dissolved
  formation survives anywhere in the runtime state. The *declared* recovery
  path deliberately stays registered with the planner that owns it, which is
  what keeps `UnknownFormation` ("torn down") distinguishable from
  `NoRecoveryPolicy` ("never declared"); the cost is that one formation id
  cannot be registered twice in the same runtime. Mission execution owns spawn
  identity, so that is its call, not this module's.
  `CombatRuntime::dissolve_formation` is the explicit form.
- **A refused tick changes nothing.** `apply` validates the whole report and
  computes the whole next state before it commits any of it, so a refused tick
  leaves the coordinator byte-identical and the caller may re-send a corrected
  report *for the same tick*. A tick that is not strictly newer than the last
  applied one is refused by name (`StaleFormationTick`) — a replay would let a
  destroyed leader come back and would elect a second leader.
- **A report must describe the whole formation.** A slot reported twice, an
  unknown slot, a different actor in a slot, an unreported member and a retired
  member brought back to life are each refused by name. A partial report is
  refused rather than reconciled because a centroid over a partial membership
  is a *different point*.
- **Nothing survives, nothing is recovered.** With no living member there is no
  shape to lead, so the tick ends in dissolution rather than electing a leader
  out of the dead.
- **Difficulty selects, it never composes.** `DifficultyRoster` maps a tier to
  the already-lowered `SkillProfile`s for that tier, and `AceVariant` names
  the tier its profile was lowered for. `resolve_profile` selects between them
  and refuses an undeclared tier (`UnknownDifficultyTier`), a tier that says
  nothing about the assigned role (`DifficultyTierMissingRole`), an unknown ace
  (`UnknownAce`), an ace of another role (`AceAssignmentMismatch`) and an ace
  lowered for another tier (`AceTierMismatch`). Composing a tier onto a profile
  is **not** done here — that belongs to the lowering boundary (#551), and
  doing it in two places is how a tier ends up meaning two things.
  Non-negotiable 1 holds by construction: a `SkillProfile` has no field a rate,
  a time scale or a damage multiplier could be expressed in, and `RoleArsenal`
  is the role's structural requirement rather than a knob, so no tier can hand
  a guns-only escort a rocket rack. There is deliberately **no baseline
  fallback**: choosing difficulty is an explicit mission decision and defaulting
  it would hide a content bug behind a plausible profile.
- **An ace is a `pilot` identity, not an actor.** `AceId(ContentId)` validates
  its namespace the way `cs_sim::allies::PilotId` does, so a pilot id can
  never be mistaken for an `ActorId`. The fixture's ace id is the same
  `pilot/synthetic.ace-wing-leader` the declared record carries
  (`cs_content::ai::declared_synthetic_ace_profile`), so the producer and the
  runtime record name one pilot.
- **One authority per session generation.** `CombatRuntime` is per-session like
  `DamageResolver`, `TargetStore` and `AlliesRoster`; a foreign member, a
  foreign assigned target and a foreign registration are all refused by
  `ForeignSession`, and a request naming a formation the observer is not a
  living member of is refused as `UnknownFormationMember`.
- **The declared→runtime lowering boundary is still not in this task's owner
  paths** — see below.

## Unknowns recorded (not guessed)

- The original game's formation shapes, slot spacing, station radius,
  succession rule after a leader is lost, regroup geometry and the trigger
  precedence are **unmeasured**; F32-A already records this and F32-D is the
  retail stage. `FORMATION_TRAIL_SPACING_M` (120 m),
  `FORMATION_STATION_RADIUS_M` (60 m), `station_offset_m`'s trailing-line
  geometry and "promote the lowest living slot" are newly authored project
  design, not measurements.
- Whether the original AI re-anchors on a regroup point or simply continues on
  the route after leader loss is unknown; this stage implements the *declared*
  policy's semantics as designed behavior, not as an observed original rule.
- Whether the original game's difficulty option offers exactly four steps, in
  this order, under these names is unknown. `DifficultyTier` is designed
  vocabulary that mirrors `cs_content::ai::DifficultyTier` field-wise; the
  retail measurement is F32-D's.
- **The declared→runtime lowering boundary (`cs_content::ai` →
  `cs_sim::ai::combat`) is still outside this task's owner paths**, exactly as
  deepseek-1 noted on F32-B. `cs_sim` cannot depend on `cs_content` and
  `crates/cs_app` is not among F32-C's owner paths, so
  `cs_content::ai` is deliberately unchanged: the declared schema already
  carries every knob the runtime reads (`RoleArsenal`, `SkillKnobs`,
  `PriorityPolicy`, `SkillKnobOverride`, `DifficultyProfile`,
  `DeclaredAceProfile`, `DeclaredFormation`, `FormationRecovery`), and adding a
  runtime-shaped record there would be new schema rather than this stage's
  integration. What this stage added is the *runtime side* that boundary will
  target: `DifficultyRoster`, `AceVariant` and `FormationRoster` each take
  already-lowered values and refuse anything incoherent. See
  `docs/findings/2026-10-03-f32-b-maneuvers-priority-and-firing-solutions.md`
  and task **#551 `F32-LOWERING`**, which now depends on this task and owns
  `crates/cs_app`.
- The **per-actor fire-discipline state** F32-B deliberately left reported
  rather than kept is still not owned by a session. `FiringSolution` reports
  `fire_discipline_ticks`, and nothing here consumes it as a cross-tick
  schedule. That is filed rather than guessed: see the follow-up tasks created
  with this change.

## Not claimed

No original-data verification, no ECS/Bevy wiring, no route *following* (that
is navigation's) and no claim that any of the above matches the original's AI.
The task awards at most **checked** status.

## Sensitivity probes (run and reverted; none committed)

Each probe was applied to the committed `crates/cs_sim/src/ai/combat.rs`,
`cargo test -p cs_sim --test ai --locked -- accept_f32_c_` was re-run, and the
file was restored with a copy afterwards. The committed tree is the green one
(21 `accept_f32_c_*` tests: 12 scenario/integration in
`accept_f32_c_combat.rs` and 9 refusal/teardown in
`accept_f32_c_combat_failures.rs`).

| probe | caught by | result |
| --- | --- | --- |
| the recovery is reported but never applied (`next.apply_action(…)` removed) | `…leader_destroyed_mid_turn_its_followers_recover`, `…regroup_recovery_anchors_the_survivors_not_the_destroyed_leader`, `…a_declared_withdraw_releases_every_station`, and 4 more | 7 failed |
| the latch check is removed (a recovery re-fires every tick) | `…a_recovery_is_answered_once_until_its_fact_recovers` | 1 failed |
| promotion picks the *highest* living slot instead of the lowest | `…leader_destroyed_mid_turn_its_followers_recover`, `…a_refused_formation_tick_changes_nothing_and_is_retryable`, `…a_retired_member_is_not_brought_back_to_life` | 3 failed |
| dissolve-on-no-survivor removed (a leader is elected out of the dead) | `…a_formation_with_no_survivor_is_torn_down_not_recovered` | 1 failed |
| an undeclared difficulty tier falls back to another tier's profiles | `…an_undeclared_difficulty_tier_is_refused_not_defaulted` | 1 failed |
| the ace/assignment role match is removed | `…an_ace_variant_is_selected_as_data_for_its_tier` | 1 failed |
| the anchor follows the *registered* leader slot even when it is destroyed | **not caught** — see below | 21 passed |

The last probe is a real gap and is recorded rather than papered over. With the
promotion in place, the registered leader slot always holds a living member by
the time stations are computed, so the two anchor expressions agree on every
path the tests reach; the distinctness is a property of `promote_leader`, not
of `resolve_anchor`. A reviewer should treat
`accept_f32_c_holding_a_shape_never_anchors_it_on_a_destroyed_leader` as the
test that actually pins the anchor's liveness requirement (it forces the
`HoldFormation` path, where the anchor deliberately becomes a
`RegroupPoint` precisely because no living leader exists), and the survivor
filter inside `resolve_anchor` as defensive rather than as the load-bearing
guard. The review re-ran this probe (see below) and the gap is still open; it
is filed as task **#557 `F32-ANCHOR-DEADLINE`**.

## Review (bunny-2, review claim `clm_6hnhgk4b0mssrtri`)

Reviewer and implementer are the same agent identity (`bunny-2`), which is
**not** independent review; the reviewer context was fresh (no memory of the
implementation session, only this branch and the task history), but a different
agent instance or model should still re-check the semantics below. Every fix
below was found by the reviewer in the committed tree, reproduced first as a
failing test, then fixed.

### Bug 1 (fixed): a latch outlived its fact and suppressed a second leader loss

`release_latches` released `LeaderLost` only when a *living* leader existed. The
answer taken for one loss therefore also silenced the next one: with the
synthetic formation, destroying slot 0 promotes slot 1, and destroying slot 1 on
the next tick was then *not* answered at all. The coordinator kept a destroyed
leader, `resolve_anchor` had no living leader to return, and the one remaining
follower was handed **no station on that tick or any later tick** — a
permanently unrecovered formation, which is the exact failure the latch exists
to prevent, reached by the opposite route. Reachable in three ticks, caught by
neither the committed tests nor the implementer's probes.

Fixed by making a latch per *fact*: `LeaderLost` is released when the fact
recovered **or** when a new one replaced it (a leader that was living before
this tick's report and is not living now). The same class of suppression
existed for `AssignedTargetDestroyed` — a formation re-tasked onto a different
already-destroyed target lost its recovery silently — so that latch is now also
released when the assigned target changed.

Tests added: `…a_second_leader_loss_is_answered_not_swallowed_by_the_first` and
`…a_second_assigned_target_loss_is_answered_too`. Both fail on the
pre-review tree (verified by reverting each clause individually) and pass after.

### Bug 2 (fixed): a request could withhold the formation its assignment declares

`CombatRuntime::step` built the `FormationFacts` from whichever formation the
request named and never compared that name with the assignment's own formation
slot. An actor the mission assigned to formation 1, decided with
`formation: None`, therefore reported no recovery path at all — the one answer
the coordinator's authority exists to prevent, reachable by omitting a field.
`step` now refuses the disagreement in both directions:
`FormationFactsOmitted` (the assignment declares one, the request names none)
and the pre-existing `FormationAssignmentMismatch` (the two name different
formations). Test added:
`…a_request_may_not_withhold_or_replace_the_formations_facts`.

This made one committed fixture incoherent rather than wrong:
`accept_f32_c_foreign_generations_never_touch_the_formation_or_the_decision`
used an assignment with *no* slot while naming a formation, i.e. exactly the
contradiction now refused one step earlier. The fixture was corrected to give
the outsider a declared slot (so the refusal under test is the membership one,
`UnknownFormationMember`, unchanged) and the second half of that test now uses
a genuinely detached assignment. No assertion was weakened, removed or
relaxed; both original assertions still stand verbatim.

### Documentation corrections (no behavior change)

- `FormationUpdate::stations` claimed "the leader never receives one"; under a
  declared regroup the anchor is the survivors' point and the whole formation,
  leader included, is stationed on it. The doc now states the actual rule.
- `FormationCoordinator::apply` listed `NoSurvivingMember` among the errors it
  reports. It cannot report it: a recovery is applied only while a survivor
  exists, so the variant is the *name of that guard* and is documented as
  unreachable through this entry point rather than claimed as an error.
- Teardown claims were corrected to say what actually survives a dissolution
  (runtime state goes; the declared recovery path stays with the planner that
  owns it, and a formation id is therefore not reusable in one runtime).
- `AnchorMode::RegroupPoint` now records *why* the point is computed once: the
  station offsets do not average to zero, so a per-tick recomputed centroid
  makes the formation chase its own centre astern forever. This is a trap for a
  later reader who reads "recomputes the regroup point" as "every tick".

### Reviewer probes (run and reverted; none committed)

Applied to the reviewed `crates/cs_sim/src/ai/combat.rs`, `cargo test -p cs_sim
--test ai --locked -- accept_f32_c_` re-run, file restored from a copy. The
committed tree is the green one (24 `accept_f32_c_*` tests: 14
scenario/integration in `accept_f32_c_combat.rs`, 10 refusal/teardown in
`accept_f32_c_combat_failures.rs`).

| probe | result |
| --- | --- |
| the recovery is reported but never applied (`next.apply_action(…)` removed) | 9 failed (the implementer measured 7; the two new tests add one each) |
| promotion picks the *highest* living slot instead of the lowest | 4 failed (the implementer measured 3) |
| the latch no longer distinguishes a new loss from a repeat | 1 failed (`…a_second_leader_loss_is_answered_not_swallowed_by_the_first`) |
| `step` no longer compares the request's formation with the assignment's | 5 failed |
| the anchor follows the *registered* leader slot even when it is destroyed | **still not caught** — 24 passed, so #557 stays open |

The last row is the implementer's documented gap, re-verified on the reviewed
tree. The review adds one fact for whoever takes #557: before the latch fix, a
destroyed registered leader under `AnchorMode::Leader` *was* reachable (it was
the bug), and the survivor filter inside `resolve_anchor` was what stopped that
tick from handing out a station anchored on a wreck. After the fix the state is
unreachable again, so the guard is defensive; that is why no test can pin it
without exposing internals.

### Reviewer checks

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D
  warnings` — exit 0.
- `cargo test --workspace --locked` — exit 0.
- `cargo test --workspace --locked -- accept_f32_c_ --include-ignored` — exit 0;
  24 tests, all `accept_f32_c_*`, all passing.
- `cargo test -p cs_sim --test ai --locked` — 65 passed (32 F32-A, 9 F32-B, 24
  F32-C-prefixed).

## Commands run (implementation session, pre-review)

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D
  warnings` — exit 0.
- `cargo test --workspace --locked` — exit 0.
- `cargo test --workspace --locked -- accept_f32_c_ --include-ignored` — exit
  0; the `ai` target ran 21 tests, all of them `accept_f32_c_*`, all passing.
- `cargo test -p cs_sim --test ai --locked` — 62 passed (32 F32-A, 9 F32-B, 21
  F32-C-prefixed).

## Notes for the owner

- `RecoveryTrigger` gained `PartialOrd, Ord` so the coordinator can hold the
  set of already-answered triggers in a `BTreeSet` and report them in a stable
  order. The order follows `RecoveryTrigger::ALL` and changes no existing
  behavior; the F32-A tests are unchanged and still pass.
- The `FormationUpdate::previous_leader` field is deliberately the leader
  *before* this tick's recovery, not "no leader" — it is what lets a consumer
  read "the lead was lost and here is who replaced it" off one record.
- `CombatStep::formation` carries the `FormationFacts` a decision was made
  against, so the consumer trace is self-describing without the caller having to
  re-query the coordinator between `step` and its own bookkeeping.
