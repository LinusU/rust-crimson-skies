# #526: the follower's bank hold, its climb hold, and what the envelope's bounds measure

Date: 2026-10-07. Task: **#526** "Give the integrated F31 follower measured
bank state and a bank-hold so a displaced actor rejoins laterally" (follow-up
to #451). Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`,
stage `### F31-C`, AC03. Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md`. Capabilities used: ordinary build/test
only — no `CS_GAME_DIR` read, no evidence report required.

#451 left two things open (`docs/findings/2026-10-02-t451-bank-sign-and-envelope-subject.md`):
the F31 follower carried **no measured bank**, so its roll command was an
undamped double integrator on an airframe with no bank holding; and the
envelope's declared turn bound was not a statement about any airframe. This
note records the cascade that closes the first and the measurements behind
what is now declared, reconciled or recorded for the second.

## 1. The follower is a cascade of two inner loops

`cs_sim::ai::navigation::NavState` gained `bank_rad` (positive
**right-wing-down**, the sign of `FlightInput.roll`), sampled in
`cs_app::ai::navigation::nav_state` from the live Avian `Rotation` as
`-atan2(right.Y, up.Y)` — the same relation
`FlightModel::bank_level_assist` measures, with the pitch axis dropping out of
both components.

`Navigator::command_for` now runs a cascade instead of mapping a heading step
straight onto a roll *rate*:

```text
outer heading loop:  omega  = clamp(GAIN * heading_error, +/- max_yaw_rate)
                     phi    = -clamp(atan(omega * V / g), +/- max_bank)
inner bank loop:     roll   = clamp((phi - bank) / (max_bank / 2), +/- 1)

outer vertical loop: climb  = height_error / CLIMB_APPROACH_S   (unchanged)
inner climb loop:    pitch  = clamp((climb - measured_climb) / 30, +/- 1)
```

with `g` read from `FlightEnvironment::SEA_LEVEL` — the same gravity the force
law applies — and `HEADING_LOOP_GAIN_PER_S = 1.0`,
`CLIMB_LOOP_FULL_SCALE_MPS = 30.0` as the two designed gains.

The pitch change was not optional. The old law (`pitch = climb / max_climb`)
is three integrators in series with no damping term, and the measured trace of
it is a **growing** oscillation. Flying the production follower and the
production airframe level, at cruise, on a marker 4 km ahead:

| t (s) | 0.5 | 1.0 | 1.5 | 2.0 | 2.5 | 3.0 | 3.5 | 4.0 | 4.5 | 5.0 | 5.5 | 6.0 | 6.5 | 7.0 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| altitude (m), before | -0.7 | -1.9 | -3.1 | -3.6 | -2.9 | -0.7 | +2.6 | +6.4 | +9.4 | +10.0 | +6.9 | -0.5 | -11.0 | -21.5 |
| altitude (m), after | -0.7 | -1.7 | -2.0 | -1.3 | -0.2 | +0.8 | +0.9 | +0.5 | 0.0 | -0.1 | +0.2 | +0.5 | +0.6 | +0.4 |

The inner climb loop turns the same run into a bounded ±2 m wobble, because
the attitude the pitch rate integrates is exactly the integrator that trims
the untrimmed synthetic airframe.

## 2. AC03 through the integrated loop: the lateral rejoin now happens

`crates/cs_app/tests/accept_t526_lateral_rejoin.rs` spawns an actor across the
route's first leg (20 m off, twice the marker's authored 10 m radius), pointed
straight down that leg, and flies the production `AiNavigationPlugin` +
`FlightForcesPlugin` + Avian fixed-tick loop until progress passes the first
mandatory marker. Measured through the same fixture with the production
follower (offset -> arrival position, meters, marker at `[0, 0, -120]`,
radius 10):

| lateral offset | before #526 | after #526 |
|---|---|---|
| 8 m  | (not run) | arrives at `(-0.36, -1.16, -110.17)` |
| 12 m | arrives at 9.9 m from the marker (marginal, altitude-dominated) | arrives at `(-0.15, -2.18, -110.58)` |
| 15 m | **no arrival**: passed the marker at 13.1 m (altitude -10 m) | arrives at `(0.33, -3.13, -110.60)` |
| 20 m | **no arrival** | arrives at `(1.28, -4.95, -111.80)` |

The lateral channel closes on its own — in every case the `X` offset is inside
the marker radius by the time the aircraft reaches the marker plane; what
decided arrival before #526 was the altitude the unstable pitch loop threw
away. The test also asserts the swept arrival is inside the marker's own
authored radius, that progress is monotonic, that no refusal is recorded, and
that the airframe never rolls past 90 degrees (the #451 held-roll symptom).

### Mutation probes (run, then restored; the tree was byte-identical after each)

* **Bank sign inverted** (`roll = (bank - phi) / ...`):
  `accept_t526_laterally_displaced_actor_rejoins_before_the_next_mandatory_marker`
  **fails** ("must reach its next mandatory marker inside the integrated tick
  budget"), and the unit test
  `accept_t526_the_measured_bank_decides_the_roll_command` **fails**.
* **Bank feedback removed** (`roll = phi / ...`, the pre-#526 shape):
  both tests **fail** the same way.
* Restored: both green, `diff` against the pre-mutation copy empty.

## 3. `ManeuverEnvelope`: what is reconciled and what is recorded

### Reconciled: the climb and dive bounds

At the cruise throttle the follower commands (0.44 spool), flying the
production airframe with the follower's own saturated vertical command:

* climb: peak **16.6 m/s**, sustained ~16 m/s over the first 6 s, while the
  airframe bleeds speed from 40 to 27 m/s doing it;
* dive: peak **35.0 m/s**, sustained 25–27 m/s.

The declared bound was `max_climb_rate_mps = 20`, which the airframe never
reached. It is now **15**, inside the measurement, so every climb the follower
can command is one the airframe actually flies; `max_dive_rate_mps = 25`
stays, inside its measurement. Both are asserted by
`accept_t526_the_declared_climb_bounds_are_flown_by_the_airframe` (peak and
last-second mean against the declared bound, climb and dive).

Recorded, not hidden: the *long-run* climb at that throttle is thrust-limited
to roughly `T·V/(m·g) ≈ 11 m/s` (4.2 kN at 40 m/s against 1200 kg); the 16.6
above is reachable over a route leg only because the airframe trades kinetic
energy for altitude. A mission that needs a sustained 15 m/s climb must command
more throttle than `step.speed / max_speed` — that is an F24/F31 tuning
question, not a #526 one.

### Recorded with measured evidence: the turn bound

`max_yaw_rate_radps = 1.0` and `max_bank_rad = pi/3` still are not the same
coordinated turn: `g·tan(pi/3)/40 = 0.424 rad/s`. What changed is that the
number is now *measured and pinned* rather than asserted away:

* the follower's **effective** turn bound is 0.424 rad/s, because the bank
  clamp in the cascade saturates long before the declared 1.0 is reached;
* the production airframe, flown by the production bank-hold cascade,
  **reaches the declared bank** (measured 1.099 rad against a declared 1.047)
  and **sustains 0.488 rad/s** there — 116% of the coordinated value, inside
  the 0.5–1.5x band the test pins. It is not a textbook coordinated turn: with
  no weathervane moment in the force law, the yaw channel holds the nose while
  the flight path swings, so the airframe turns this one with ~30 degrees of
  sideslip and a descending spiral (measured sideslip 0.55 rad at t=3 s);
* the declared kinematic `1.0 rad/s` is **not reachable** by this airframe
  (measured 0.488), and that is pinned by
  `accept_t526_the_declared_turn_bound_is_the_turn_the_airframe_sustains`
  rather than deleted.

Why not simply derive the number: it was tried, measured and reverted.
Deriving `max_yaw_rate_radps = 0.424` makes the kinematic turn radius 94 m
instead of 40 m, and the designed arch fixture then stops flying: `cs-inspect
routes follow` reports `reached: 1, rejoined: false` over its whole 20000-tick
budget where it used to rejoin, because the wider turn no longer fits between
the route's nodes and its wall. The 40 m radius is the reason the bound exists
(`synthetic_maneuver_envelope`'s own doc), so the value stays the kinematic
**command contract** and the mismatch with the airframe is recorded here and
in the envelope's documentation. Reconciling it for real needs an envelope
that separates the kinematic contract from the airframe bound — or an
assisted airframe with cruise trim — and that is a change to the F31 contract
itself, not to this task's owner paths.

## 4. What changed

* `crates/cs_sim/src/ai/navigation.rs`: `NavState::bank_rad` (+ its finiteness
  validation), `HEADING_LOOP_GAIN_PER_S`, `CLIMB_LOOP_FULL_SCALE_MPS`,
  `coordinated_bank_rad`, the cascade in `Navigator::command_for` (heading
  error is now passed in), the kinematic closures in `follow_route` and
  `SyntheticArchProbe::run` carrying the bank their own committed turn
  implies, `synthetic_maneuver_envelope`'s `max_climb_rate_mps` 20 -> 15 plus
  the measurement record, and the two `accept_t526_` unit tests.
* `crates/cs_app/src/ai/navigation.rs`: `nav_state` samples the measured bank
  from the live `Rotation`.
* `crates/cs_sim/tests/accept_t526_bank_hold_and_envelope.rs` (new): the turn,
  climb and dive measurements through the production follower + flight model.
* `crates/cs_app/tests/accept_t526_lateral_rejoin.rs` (new): AC03 through the
  production Avian fixed-tick loop.
* `tools/cs_inspect/src/routes.rs` and the two `NavState` literals in the
  `cs_sim`/`cs_inspect` test suites: the added field, no behaviour change.
  `crates/cs_sim/tests/accept_f31_a_navigation.rs`'s envelope test is back to
  its original assertion (the derived-rate experiment was reverted).

No protected path was touched, no arrival radius was inflated, no test was
weakened or skipped.

## 5. Commands run

From the repository root (exit codes as printed).

Full set, on the pre-rebase tree:

```
cargo fmt --all -- --check                                                   -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                              -> 0
cargo test --workspace --locked -- accept_t526_ --include-ignored            -> 0 (5 tests selected, all passed)
```

The branch was then rebased onto `origin/main` (22 incoming commits,
`ac99d514`) with no conflicts; the incoming commits touch none of the files
this branch changes and no `Cargo.toml`/`Cargo.lock`, so the owner's
2026-10-01 merge-race directive's lighter check set applies to the rebased
tree (base `ac99d514`):

```
cargo fmt --all -- --check                                                   -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked -- accept_t526_ --include-ignored            -> 0 (5 tests selected, all passed)
```

CI runs the plain workspace test run on the pushed commit.

## 6. Evidence

Synthetic fixtures and design only: no original-data, visual, audible or
ordinary-play claim, so this stage can award at most **checked**. No evidence
report is produced because #526 needs no capability beyond plain build/test
and makes no fidelity claim. The original route encoding, AI cadence and the
original game's own bank behaviour remain unmeasured (F31-D).

## 7. Sources

- `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (AC03, `### F31-C`).
- `docs/contracts/FLIGHT-PHYSICS.md` (axes, one gravity, rate-command attitude
  control, calibration acceptance).
- `docs/findings/2026-10-02-t451-bank-sign-and-envelope-subject.md` (#451's
  defect record and this task's brief).
- `crates/cs_sim/src/ai/navigation.rs` (`NavState`, `ManeuverEnvelope`,
  `Navigator::command_for`, `follow_route`, `synthetic_maneuver_envelope`).
- `crates/cs_sim/src/flight/model.rs` (`body_torque`, `bank_level_assist`),
  `crates/cs_sim/src/flight/synthetic.rs` (`synthetic_fixed_wing`).
- `crates/cs_app/src/ai/navigation.rs` (`nav_state`, `AiNavigationPlugin`),
  `crates/cs_app/tests/accept_t446_ai_navigation_wiring.rs` (the test pattern).
- `tools/cs_inspect/src/routes.rs` (`build_follow_report`, the rejoin report).
