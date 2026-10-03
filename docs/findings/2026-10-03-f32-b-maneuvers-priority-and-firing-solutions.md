# F32-B: Maneuvers, target priorities and firing solutions

Date: 2026-10-03. Task: F32-B "Implement maneuvers, target priorities and
firing solutions" (`specs/F32-ai-combat-formations-aces-and-difficulty.md`,
section `### F32-B`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only (no `CS_GAME_DIR` read, no evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/ai/combat.rs` (owner path): the F32-B runtime half.
  `CombatManeuver` (the typed intent a role flies, selected from the role
  and whether a target was chosen), `FiringSolution` / `MountFire` /
  `MountFireState` / `FireHoldReason` (the per-mount firing solution), the
  `CombatPlanner::firing_solution` per-session entry point, the
  `DecisionTrace::maneuver` and `DecisionTrace::firing` fields, and the
  synthetic `synthetic_bomber_profile` / `synthetic_ace_bomber_profile`
  fixtures.
- `crates/cs_sim/tests/ai/accept_f32_b_combat.rs` (new, owner path): the
  nine `accept_f32_b_*` runtime acceptance tests.
- `crates/cs_sim/tests/ai/main.rs` (owner path, wiring only): declares the
  new test module and updates the target's doc comment.
- This file.

**One observable failure:** the minimum scenario. An ordnance-carrying ace
(a bomber variant that may fire both a gun and a rocket rack) with a
*disabled* gun mount (its F29 damage node destroyed) and an *intact but
empty* rack must fire neither. A firing solution that ignored mount
availability, or that treated "high skill" as permission to fire, would
report a shot from one of them.
`accept_f32_b_ace_cannot_fire_a_disabled_gun_or_an_empty_rocket_rack`
asserts both refusals by name; its failure cases show the same unavailable
arsenal firing for nobody and a loaded arsenal firing for both the ace and
the slower bomber.

## Semantics defined at this stage

- **Target priority was already the F32-A policy function.** F32-A's
  `CombatPlanner::decide` already ranks declared hostiles with the four
  weighted terms and the reaction/threat-window gates. F32-B does not
  rewrite it; it consumes the chosen target.
- **Maneuver selection is separate from role selection.**
  `CombatManeuver::select(role, has_target)` is the total, deterministic
  map from "what a script assigned" plus "is there a target" to what the
  role actually flies this tick: `Engage`, `AttackRun`, `Screen`,
  `BreakAway`, `Withdraw` or `Hold`. It names no waypoint, heading or
  control law, so choosing a maneuver cannot make the planner a second
  pose owner (`docs/contracts/FLIGHT-PHYSICS.md`). `Evade` always breaks
  away and `Retreat` always withdraws; an escort screens when it has
  nothing to answer and closes when it does.
- **Firing solutions are per-mount and availability is not skill.**
  `FiringSolution::solve` classifies every mount in the
  `ArsenalSnapshot`: a kind the role's `RoleArsenal` does not declare is
  `WrongKind`, otherwise a destroyed mount is `Disabled`, otherwise an
  out-of-rounds mount is `Empty`, otherwise a mount still on its cadence is
  `CoolingDown`, otherwise it `Firing`s. Precedence is fixed, so the
  reported reason is a function of the mount, not of the snapshot's order.
  A disabled gun and an empty rocket rack are therefore two distinct
  refusals, and a high-skill profile cannot fire either one (AC02).
- **A hold names the cause.** When nothing fires,
  `FiringSolution::hold()` returns `RoleUnarmed` for a role with no
  declared weapon, or `NoUsableMount { disabled, empty, cooling,
  wrong_kind }` with the count of each distinct refusal — so destruction,
  exhaustion and cadence stay distinguishable in the record rather than
  collapsing into "no shot".
- **Skill reaches the solution only as behavior.** The solution carries
  the effective profile's `aim_error_rad` and `fire_discipline_ticks`.
  The per-mount `cooldown_ticks` the weapon system owns is enforced here;
  the actor-level discipline across ticks needs state, so the solution
  reports it for the stateful consumer (F32-C) instead of a declared knob
  silently disappearing.
- **Line of fire stays separate from availability** (non-negotiable 3).
  The friendly-in-line-of-fire veto is a property of the selected
  candidate and remains `CandidateTrace::fire_veto`; the firing solution
  never folds it in, so "my mount may fire" and "the line to my target is
  clear" remain two answers.
- **The per-session authority holds.** `CombatPlanner::firing_solution`
  refuses a target from another session generation by name
  (`CombatError::ForeignSession`), the same rule the planner applies
  everywhere else, even though the classification itself only reads the
  profile and the snapshot.
- **`decide` emits both.** `DecisionTrace` now carries `maneuver` and, when
  a target was chosen and an arsenal was supplied, `firing`. With no
  eligible candidate the maneuver is the "no target" one and no firing
  solution is invented. F32-A's tests are unchanged and still pass.

## Unknowns recorded (not guessed)

- The original game's AI maneuver set, its firing cadence, hardpoint
  firing order and aim error are **unmeasured**; F28-C and F32-A already
  record this and F32-D is the retail stage. Every maneuver label, refusal
  reason and fixture value here is newly authored project design with
  designed provenance, not original data.
- Whether the original AI ever fires a disabled or empty mount (it does
  not in any sane model, but the *rule* is unmeasured) is unknown; this
  stage refuses it as designed behavior, not as an observed original rule.
- **The declared→runtime lowering boundary for roles and priority is not
  in this task's owner paths.** F32-A's finding expected the
  `cs_content::ai` → `cs_sim::ai::combat` lowering to live in
  `cs_app::ai::combat` and to be F32-B's. F32-B's owner paths are
  `crates/cs_sim/src/ai/combat.rs`, `crates/cs_content/src/ai.rs`,
  `crates/cs_sim/tests/ai/` and `docs/findings/`; `crates/cs_app` is not
  among them and `cs_content` may not depend on `cs_sim`. The lowering is
  therefore not done here. It should be picked up where F32-C wires the
  producer to the consumer (noted on F32-C, #131), not by widening this
  task's owner paths.
- `crates/cs_content/src/ai.rs` is an owner path of this task but the
  declared schema already carries every knob the firing solution reads
  (`RoleArsenal`, `aim_error_rad`, `fire_discipline_ticks`), so it is
  deliberately unchanged: adding a fired/refused *record* to the declared
  side would be new schema, not this stage's runtime implementation.

## Not claimed

No original-data verification, no ECS/Avian wiring and no stateful
formation recovery — F32-C wires the maneuver and firing output into the
session and F32-D measures the original. The task awards at most
**checked** status.

## Sensitivity probes (run and reverted; none committed)

Each probe was applied to the committed
`crates/cs_sim/src/ai/combat.rs`, `cargo test -p cs_sim --test ai --locked
-- accept_f32_b_` was re-run, and the file was restored with
`git checkout` after every probe. The committed tree is the green one.

| probe | caught by | result |
| --- | --- | --- |
| the `disabled` branch can never fire (`false && mount.disabled`) | `accept_f32_b_ace_cannot_fire_a_disabled_gun_or_an_empty_rocket_rack`, `…firing_availability_is_independent_of_shooter_skill`, `…firing_solution_names_each_refusal_kind`, `…decide_reports_the_maneuver_and_the_firing_solution` | 4 failed |
| the `rounds == 0` branch can never fire (`false && mount.rounds == 0`) | `…ace_cannot_fire_a_disabled_gun_or_an_empty_rocket_rack`, `…firing_availability_is_independent_of_shooter_skill`, `…firing_solution_names_each_refusal_kind`, `…decide_reports_the_maneuver_and_the_firing_solution` | 4 failed |
| cadence no longer defers (`false && mount.cooldown_ticks > 0`) | `…a_cooling_mount_defers_without_being_unavailable`, `…firing_solution_names_each_refusal_kind` | 2 failed |
| the role-kind rule is dropped (`false && !allowed`) | `…firing_solution_names_each_refusal_kind`, `…unarmed_roles_hold_fire_whatever_the_snapshot_carries` | 2 failed |
| the `RoleUnarmed` hold is dropped | `…unarmed_roles_hold_fire_whatever_the_snapshot_carries` | 1 failed |
| escort with no target screens → holds instead | `…maneuver_follows_the_role_and_the_situation`, `…no_target_reports_the_no_target_maneuver_and_no_firing` | 2 failed |
| the foreign-target session check is dropped | `…firing_solution_refuses_a_foreign_target` | 1 failed |

## Commands run

- `cargo fmt --all -- --check` — clean.
- `cargo clippy -p cs_sim --all-targets --all-features --locked -- -D warnings`
  — clean. (Workspace-wide clippy and test results are recorded in the
  handover.)
- `cargo test -p cs_sim --test ai --locked` — 41 passed (32 F32-A, 9
  F32-B).
- `cargo test -p cs_sim --test ai --locked -- accept_f32_b_` — 9 passed.

## Notes for the owner

- The task's title names "target priorities", but F32-A already implemented
  the priority policy function; F32-B consumes it rather than duplicating
  it. This is called out above so a reviewer can confirm the split.
- The firing solution is a pure classifier over validated inputs; the
  stateful per-actor cadence (`fire_discipline_ticks`) is the only part
  left for a session to keep and is reported for that purpose.
