# F31-C ECS wiring: the integrated follower does not fly the real airframe

Date: 2026-10-01. Task: #446 "Wire the F31 navigation driver into the mission
ECS and bind moving anchors" (follow-up to F31-C #127). Spec:
`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (stage `### F31-C`,
AC03/AC04). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

This note records a defect the ECS wiring exposes in the F31-A consumer half.
#446 owns the `cs_app` ECS wiring, not `crates/cs_sim/src/ai/navigation.rs`, so
the defect is recorded and filed as a follow-up rather than fixed here (AGENTS
rule 1). It is **not** hidden: the #446 rejoin test is deliberately authored on
the one control axis that really converges through the integrated loop, and the
lateral axis is left unclaimed.

## What #446 adds

- `crates/cs_app/src/ai/navigation.rs` (new): `bind_route` (content ->
  runtime), `BoundRoute` (authored id -> runtime id), `MovingAnchor`,
  `RoutePursuit`, `AiNavigation`, `AiNavigationPlugin` and the two `FixedPreUpdate`
  systems that own the `NavigationSet`, feed it the live Avian state and sample
  the live anchor each tick.
- `crates/cs_app/src/ai/mod.rs`, `crates/cs_app/src/lib.rs` (wiring/doc only).
- `crates/cs_app/tests/accept_t446_ai_navigation_wiring.rs` (new): 8
  `accept_t446_` tests.
- This file.

## The gap, with measured evidence

Two independent effects stop the F31 follower from flying the synthetic
airframe as its envelope assumes. Both were measured through the production
path (the real `PhysicsFixture` + `FlightForcesPlugin` + `AiNavigationPlugin`,
`cs_content` route -> `bind_route` -> `NavigationSet` -> `FlightAircraft`).

1. **The bank command has the wrong sign for a turn.** `Navigator::command_for`
   maps a desired *yaw* step to `FlightInput.roll`:

   ```rust
   let roll = turn_fraction * (self.envelope.max_bank_rad / FRAC_PI_2);
   ```

   `turn_fraction` is positive for a nose-left heading change (canonical
   `+Y`, positive heading). `docs/contracts/FLIGHT-PHYSICS.md` and
   `accept_f24_a_control_axes_map_to_their_body_axes` define positive
   `FlightInput.roll` as **right-wing-down** (a right-hand rotation about body
   forward `-Z`), which turns the aircraft **nose-right**. So a positive yaw
   step commands the wrong bank.

   Measured: an actor displaced to `[60, 0, -40]` with the marker at
   `[0, 0, -120]` (desired heading `+0.6435 rad`) is commanded a constant
   `roll = +0.667`. Its true body heading drifts `0 -> -0.328 rad` (nose-right,
   away from the marker) over 400 ticks, and its lateral offset grows from 60 m
   to 65 m. A temporary experiment that flipped the roll sign in the ECS apply
   step made the heading drift the other way (`+0.328 rad`) toward the marker,
   which confirms the sign — not the frame or the ECS sampling — is the cause.
   (`cs_inspect routes --follow` never caught this because F31-C drives a
   kinematic closure in which the committed heading step is applied directly,
   so `command.roll` is never integrated.)

2. **Even with the sign corrected, the closed loop cannot turn or hold altitude
   as the envelope assumes.** The declared envelope allows
   `max_yaw_rate_radps = 1.0`; the synthetic airframe, banking at the commanded
   `max_bank_rad = pi/3`, turns at roughly `0.1 rad/s`. A lateral displacement
   of `D` m closes by only a few metres over the 120 m fixture leg
   (`D = 20 -> 19`, `40 -> 37`), and lengthening the leg does not help because
   the airframe cannot hold altitude with a zero-pitch command (its cruise
   trim angle of attack is ~0.094 rad): over a 300-500 m leg the aircraft sinks
   24-83 m, so the 3D closest approach is dominated by the vertical error.

**Consequence.** AC03 ("an AI displaced off route rejoins before the next
mandatory marker") cannot be demonstrated through the integrated flight loop
for a lateral displacement, and no honest arrival radius (the F31-C fixtures
use 3-12 m) makes it a real rejoin. #446's test therefore displaces the actor
**vertically** (15 m below the route): the production pitch/climb channel
genuinely closes that offset to 1.3 m of the marker (`reach` at tick 208). The
lateral/bank channel is left as an open limitation.

## Unknowns and limitations (affected content, resolving task)

Affected content: every AI aircraft following a route in the integrated loop —
i.e. the whole F31 navigation path once a mission places an AI actor. Resolving
task: **#451 "Fix the F31 follower's bank-command sign and calibrate the
integrated turn/altitude loop"** (filed with `create_tasks` from this session,
depends on #446). It must (a) make a positive yaw step
command the bank that turns toward it, with a closed-loop test that fails when
the sign is inverted, and (b) reconcile the declared `ManeuverEnvelope` with the
airframe it is flown by (or record, with evidence, that the F24 synthetic
airframe is not the envelope's subject) so a displaced actor rejoins laterally
within a physically motivated arrival radius. Until then the following stay
**unknown / unachieved** and must not be claimed:

- lateral/displaced route rejoin through the integrated flight loop;
- the real turn radius and climb authority of any airframe against its declared
  envelope.

The ECS wiring itself (#446) is unaffected: registration, live moving-anchor
sampling, monotonic set-owned progress, teardown and session confinement are
all exercised by the passing tests.

## Mutation probe (the tests fail when the wiring is removed)

One probe was applied to the working tree, run, and reverted; it is not
committed.

1. `AiNavigationPlugin::build` made a no-op (no `AiNavigation`,
   `NavigationTickReport` or `PendingNavigationCommands`, no systems) ->
   `cargo test -p cs_app --test accept_t446_ai_navigation_wiring` FAILED with
   6 of 8 tests panicking on the missing `AiNavigation` resource. The two that
   still pass (`bind_route_refuses_unbound_anchor_and_loop`,
   `ai_driver_writes_only_the_flight_command_boundary`) exercise the pure
   `bind_route` boundary and the argument boundary, not the running driver.

The remaining sensitivities follow directly from the code and are the reviewer's
to reproduce against the committed tree: making `bind_route` return
`RouteFrame::World` drops the anchor binding (the moving-anchor tests and the
unbound-anchor refusal fail), and dropping the roster reconciliation leaves a
despawned actor's state registered (the teardown test fails).

## Commands run

From the repository root; exit codes as printed on the final tree:

```
cargo fmt --all -- --check                                               -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                          -> 0
cargo test --workspace --locked -- accept_t446_ --include-ignored        -> 0 (8 tests selected, all passed)
```

## Evidence

Synthetic fixtures and design only. No original-data, visual, audible or
ordinary-play claim; this stage can award at most **checked**. No evidence
report is produced because #446 needs no capability beyond plain build/test and
makes no fidelity claim. F31-D owns retail route-coverage evidence.

## Sources

- `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (AC01-AC04,
  `### F31-C`), `docs/contracts/FLIGHT-PHYSICS.md`,
  `docs/01-ARCHITECTURE.md`.
- `crates/cs_sim/src/ai/navigation.rs` (`ManeuverEnvelope`, `Navigator::command_for`),
  `crates/cs_sim/src/flight/synthetic.rs`, `crates/cs_sim/src/flight/model.rs`
  (`CONTROL_AXIS`, `lift_coefficient`), `crates/cs_app/src/physics/flight.rs`.
- `crates/cs_app/tests/accept_f24_c_flight_wiring.rs` (roll/turn conventions),
  `crates/cs_sim/src/flight/model.rs`
  (`accept_f24_a_control_axes_map_to_their_body_axes`).
- `docs/findings/2026-10-01-f31-c-original-routes-and-moving-frames.md`
  (limitation 1: no ECS / integrated-flight-loop wiring; limitation 2:
  content-id -> runtime-id binding).
