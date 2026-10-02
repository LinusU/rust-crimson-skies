# #451: the F31 bank-command sign, and why the F24 synthetic airframe is not the envelope's subject

Date: 2026-10-02. Task: **#451** "Fix the F31 follower's bank-command sign and
calibrate the integrated turn/altitude loop" (follow-up to #446). Spec:
`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (stage `### F31-C`,
AC03). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities used:
ordinary build/test only (no `CS_GAME_DIR` read, no evidence report required).

This note records (1) the fix for the bank-command sign and the regression tests
that pin it, and (2) the measured evidence behind the task's alternative branch:
**the F24 synthetic airframe is not the subject of the declared
`ManeuverEnvelope`**, so the integrated-loop lateral rejoin is recorded as an
open limitation rather than papered over.

## 1. Defect 1 — the bank command had the wrong sign (fixed)

`cs_sim::ai::navigation::Navigator::command_for` mapped a desired *yaw* step to
`FlightInput.roll` as `roll = +turn_fraction * (max_bank / FRAC_PI_2)`.
`turn_fraction` is positive for a nose-left step (canonical `+Y`, positive
heading), but `docs/contracts/FLIGHT-PHYSICS.md` and
`accept_f24_a_control_axes_map_to_their_body_axes` define positive
`FlightInput.roll` as **right-wing-down**, which turns the aircraft
**nose-right**. A positive step therefore commanded the opposite bank and the
follower banked away from a lateral target.

The fix is one sign:

```rust
let roll = -turn_fraction * (self.envelope.max_bank_rad / std::f64::consts::FRAC_PI_2);
```

Two tests pin the sign:

- `accept_t451_a_heading_error_commands_the_bank_that_turns_toward_it` (unit,
  in `crates/cs_sim/src/ai/navigation.rs`): a target to the left must produce a
  negative roll and a target to the right a positive one.
- `accept_t451_a_integrated_follower_turns_toward_the_target_it_commands`
  (new, `crates/cs_sim/tests/accept_t451_integrated_bank_sign.rs`): drives the
  production `Navigator::decide` command into the production `FlightModel`
  (`synthetic_fixed_wing`) through a fixed-tick semi-implicit Euler integrator
  (the same body integration `cs_sim::probes::ProbeRunner` uses) and asserts
  the integrated body heading turns toward the target side. The test also flies
  the explicitly negated roll command and asserts it turns the other way, so the
  discriminating mutation is built into the test.

**Mutation probe.** Reverting the fix (`roll = +turn_fraction * …`) and running
both tests fails both: the unit test reports `got 0.6666` for the left target,
and the integrated test reports a heading of `-0.0097` (nose-right) for an
actor whose target is on its left. The working tree was restored.

## 2. Defect 2 — the F24 synthetic airframe is not the envelope's subject

The declared envelope
(`cs_sim::ai::navigation::synthetic_maneuver_envelope`) has
`max_yaw_rate_radps = 1.0` and `max_bank_rad = pi/3`. Those two numbers are not
mutually consistent as a coordinated-turn model either: a coordinated turn at
`pi/3` and the declared cruise speed of 40 m/s is
`g·tan(pi/3)/V = 9.80665·1.7321/40 ≈ 0.424 rad/s`, not 1.0. The envelope is a
**command contract** chosen so the 40 m synthetic arch route is flyable
(40 m turn radius, small against the node spacing), not a measurement of an
airframe.

The F24 synthetic airframe is a fidelity model with a **rate-command roll
channel and no bank holding and no trim**:

- **Committed evidence test**
  `accept_t451_the_f24_synthetic_airframe_does_not_hold_a_bank`
  (`crates/cs_sim/tests/accept_t451_integrated_bank_sign.rs`) flies the
  production `FlightModel` with a **held** `roll = 1.0` for five synthetic
  seconds and asserts the body-up axis passes below zero, i.e. the rate command
  carried the body past 90° instead of settling at the envelope's
  `max_bank_rad = pi/3`. `body_torque` builds `desired_rate = command *
  max_rate_radps[roll]`, and `bank_level_assist` is off in the CALIBRATED
  profile, so there is no restoring roll moment; a zero-pitch command also does
  not hold altitude because the airframe needs its cruise trim angle of attack
  (the same gap #446 recorded).
- The F31 follower carries **no measured-bank state** (`NavState` is position,
  heading, speed, climb). Mapping a heading error to a roll *rate* is a
  double-integrator loop with no damping term, so even with the sign fixed the
  commanded bank grows without bound: the committed integrated sign test only
  holds its 120-tick (1 s) horizon, and an *exploratory throwaway probe* (not
  committed) observed the bank pass 90° and the actor's lateral offset grow from
  60 m to hundreds of metres. Raising or lowering the envelope's
  `max_yaw_rate_radps` does not help, because only `max_bank_rad` sets the
  (saturating) roll command.
- An *exploratory throwaway* **bank-hold** prototype (heading error → desired
  turn rate → desired bank via `atan(ωV/g)` → roll from measured bank) with
  perfect bank feedback closed a **30 m** lateral offset to only **17.6 m** —
  never inside the fixture's 10 m marker radius — over the 120 m leg, but closed
  a **15 m** offset to **9.7 m** and reached the marker. That is the signature of
  the missing follower state, not of the equations: the airframe can fly a small
  offset once the follower can hold a bank angle, and this task's owner path has
  nowhere to put that state.

**Consequence.** Through the F24 / Avian integrated loop, and with only the
`crates/cs_sim/src/ai/navigation.rs` owner path this task has, a laterally
displaced actor does **not** rejoin, and no honest arrival radius (the F31-C
fixtures use 3-12 m) changes that. The production *kinematic* follower loop does
rejoin laterally — `accept_f31_c_displaced_actor_rejoins_before_the_next_mandatory_marker`
(already committed) drives `follow_route` with a 40 m displacement and reaches
the first mandatory marker — and that is the loop whose command bounds the
envelope actually describes. The F24 synthetic airframe is therefore **not the
envelope's subject**, and the integrated-loop lateral rejoin stays **unknown /
unachieved** and must not be claimed.

Resolving it needs follower state that does not exist in the owner path:
measured bank on `NavState` plus a bank-hold controller, wired through
`crates/cs_app/src/ai/navigation.rs` (a `cs_app`/ECS owner path), or an assisted
airframe with a bank-level assist and cruise trim. Both are outside this task's
`crates/cs_sim/src/ai/navigation.rs` owner path (see #446's and #457's
descriptions, which state the ECS is not an F31 owner path); the bank-hold work
is filed as **#526**.

## 3. What changed

- `crates/cs_sim/src/ai/navigation.rs`:
  - the sign fix in `Navigator::command_for`, with a comment naming the axes and
    the contract;
  - `synthetic_maneuver_envelope`'s documentation now states the envelope is a
    designed command contract and that the F24 synthetic airframe is not its
    subject, pointing at this note;
  - the new `accept_t451_` unit test.
- `crates/cs_sim/tests/accept_t451_integrated_bank_sign.rs` (new): the
  closed-loop sign test and the held-roll airframe evidence test.
- This file.

No envelope numeric value changed (the recorded branch was taken); no protected
path was touched; no arrival radius was inflated.

## 4. Commands run

From the repository root, on the final tree (exit codes as printed):

```
cargo fmt --all -- --check                                               -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                          -> 0
cargo test --workspace --locked -- accept_t451_ --include-ignored        -> 0 (3 tests selected, all passed)
```

Mutation probe (sign reverted, then restored): both sign tests fail — the unit
test reports `got 0.6666` for the left target and the integrated test reports a
nose-right heading for an actor whose target is on its left. The held-roll
airframe evidence test does not depend on the follower sign and stays green.

## 5. Evidence

Synthetic fixtures and design only. No original-data, visual, audible or
ordinary-play claim; this stage can award at most **checked**. No evidence
report is produced because #451 needs no capability beyond plain build/test and
makes no fidelity claim. The lateral rejoin through the integrated flight loop
remains open; that is recorded here rather than hidden.

## 6. Sources

- `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (AC03, `### F31-C`),
  `docs/contracts/FLIGHT-PHYSICS.md`.
- `crates/cs_sim/src/ai/navigation.rs` (`ManeuverEnvelope`, `NavState`,
  `Navigator::command_for`, `synthetic_maneuver_envelope`).
- `crates/cs_sim/src/flight/model.rs` (`CONTROL_AXIS`, `body_torque`,
  `bank_level_assist`), `crates/cs_sim/src/flight/synthetic.rs`
  (`synthetic_fixed_wing`), `crates/cs_sim/src/probes/runner.rs` (the fixed-tick
  measurement integrator the new test mirrors).
- `crates/cs_app/src/ai/navigation.rs` (`nav_state`), `crates/cs_app/tests/accept_t446_ai_navigation_wiring.rs`.
- `docs/findings/2026-10-01-f31-ecs-integrated-follower-gap.md` (the defect this
  task closes and the gap it records).
