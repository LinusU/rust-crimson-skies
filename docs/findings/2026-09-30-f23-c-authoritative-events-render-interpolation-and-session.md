# F23-C: authoritative events, render interpolation and the physics session

Date: 2026-09-30. Task: F23-C "Integrate authoritative events and render
interpolation"
(`specs/F23-avian-integration-collision-and-fixed-step-authority.md`, section
`### F23-C`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/physics/session.rs` (new): `PhysicsSession` — the one
  object a game loop owns. Producer calls (`spawn`, `submit`, `set_mode`,
  `despawn`) go through it; `pump_frame` advances `Time<Fixed>` by the
  render frame's span one tick per world update and returns a
  `SessionFrame` carrying every tick's classified contact reports, the
  spawn-preflight events and the request counters. `teardown`/`restart`
  drop and rebuild the world; every call after teardown returns
  `PhysicsSessionError::Inactive`.
- `crates/cs_app/src/physics/preflight.rs` (new): `SpawnPreflight`,
  `SpawnPreflightEvent`, `SpawnPreflightLog`, `resolve_spawn_preflights` —
  the repair for F23-B limitation 1 (see below).
- `crates/cs_app/src/physics/body.rs`: `spawn_body` binds
  `TransformInterpolation` to simulated bodies and binds the declared
  `Mass` for *every* mode that declares one (see "The mass ordering bug
  the fixture hid"); `set_body_mode` keeps interpolation in step with the
  new mode and refuses a dynamic release with no declared mass
  (`BodyTransitionError::MissingDynamicMass`).
- `crates/cs_app/src/physics/contacts.rs`: `record_contact_reports` prunes
  the active-pair set when a body (or its `BodyLayer`) is gone — the
  retention decision F23-B left to this stage (limitation 2); new
  `ContactReports::active_len` observability hook. `PhysicsBodiesPlugin`
  additionally installs the preflight channel.
- `crates/cs_app/src/physics/mod.rs`: module declarations and re-exports.
- `crates/cs_app/tests/physics/{main,session}.rs`: the `accept_f23_c_*`
  acceptance tests.
- This file.

**One observable failure:** release a kinematic aircraft to dynamic flight
and submit a force — the first tick integrates it at unit mass instead of
the declared 10 kg (measured: 120 N produced Δv = 1.0 m/s instead of 0.1).
The F23-A/B building blocks were all present but nothing wired them into a
producer/consumer path, so this had never been observable.

## The mass ordering bug the fixture hid

Two separate insertion-order defects of the same shape, both invisible to
the F23-A/B tests because `PhysicsFixture` spawns its one body as a single
bundle while `spawn_body` inserts components in stages:

1. `spawn_body` inserted `Collider` **before** `Mass`. Avian 0.7 recomputes
   `ComputedMass` when the collider arrives; at that point the declared
   `Mass` was not on the entity, so `ComputedMass.inverse` stayed `1.0`
   until the first tick's solver pass. A force applied on that tick
   integrated with unit mass. Fixed by inserting `Mass` before `Collider`.
   A 20 kg body now yields Δv = 0.05 m/s for a 120 N tick.
2. A *kinematic* `BodySpec` never bound `Mass` at all, so releasing it to
   dynamic fell back to the collider-density default — the same wrong mass
   through the transition path. `spawn_body` now binds the declared mass
   whenever the spec carries a positive one, and `set_body_mode` refuses
   `Dynamic` for a body without it (`MissingDynamicMass`) — error
   propagation instead of a silently wrong integration.

## Measured behavior on the pinned pair

`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(1)`, 120 Hz, zero gravity.
All outcomes asserted by the `accept_f23_c_*` tests.

* **AC03 release continuity.** A kinematic aircraft at 12 m/s: releasing it
  leaves `Position`/`LinearVelocity` bit-identical across the switch, the
  first dynamic tick advances `x` by `v·dt` from the scripted trajectory's
  endpoint, and a submitted force then integrates with the declared mass —
  the body is the same body, not a respawn.
* **Static → dynamic keeps stored velocity** by the same rule: the
  transition never rewrites state. Whether gameplay wants a zeroed release
  is the caller's decision (F23-B limitation 4 resolved by keeping the rule
  explicit), not a hidden write.
* **Render interpolation.** Simulated bodies carry Avian's
  `TransformInterpolation` (`PhysicsInterpolationPlugin` is already inside
  `PhysicsPlugins::default()`): a render frame half a tick long runs no
  fixed step, leaves `Position` exactly on the tick pose and eases
  `Transform.translation` between the previous and current tick poses. The
  presented pose therefore lags the simulation by up to one tick — inherent
  to interpolation; extrapolation was measured as available but not chosen.
* **First-tick tunneling closed at spawn.** F23-B measured that a swept
  body's AABB is only written at the end of its first tick. A body on a
  swept layer (`Projectile`, `Trigger`) spawned already moving now carries
  `SpawnPreflight`; `resolve_spawn_preflights` runs in `FixedPostUpdate`
  before `PhysicsSystems::Prepare` — inside the tick, before the body's
  first integration — and shape-casts one tick of velocity against the
  collider tree. A hit the declared matrix calls a *solid* contact clamps
  the spawn to the contact point (deferred via `Commands`, so it lands at
  the sync point before `Prepare`); a sensor or a non-interacting collider
  never stops the cast. Measured: a projectile at 240 m/s (2 m/tick)
  spawned 1.5 m short of a wall clamps to the wall face and produces the
  `SolidContact` report the same way an in-flight hit does; the same spawn
  crossing only a sensor flies through (`clamped = false`).
* **Report retention.** `ContactReports` keeps the current tick's batch;
  `PhysicsSession::pump_frame` drains it per tick into the frame's list so
  a multi-tick frame cannot overwrite an early tick's reports, and prunes
  the active-pair set for despawned bodies — Avian does not guarantee a
  `CollisionEnd` for a collider removed mid-contact (F23-B limitation 2).
* **Teardown/retry.** `teardown` drops the `App`; all calls then fail
  `Inactive`. `restart` rebuilds identically: tick 0, empty queues, no
  retained active pairs — the same "a second run cannot inherit the first"
  contract `run_synthetic` documents.

## Removal/mutation checks (run on this branch, reverted)

| Mutation | Result |
| --- | --- |
| `TransformInterpolation` skipped in `spawn_body` | `accept_f23_c_presentation_transform_interpolates_between_fixed_poses` fails — the transform stays tick-quantized |
| `SpawnPreflight` marker never inserted in `PhysicsSession::spawn` | `accept_f23_c_fast_spawn_preflight_clamps_onto_the_contact` and `accept_f23_c_preflight_never_stops_on_a_sensor` fail — no event, and no clamped spawn |

## Design decisions (declared, not original)

* The session is the single-clock authority (F23-A limitation 3): it
  installs `Time<Fixed>` at the declared rate and is the only writer of
  `TimeUpdateStrategy::ManualDuration`.
* The preflight *clamps* a spawn onto the contact rather than refusing it:
  the projectile reaches the wall either way, the clamp just stops it
  arriving inside it. The correction is always recorded in
  `SpawnPreflightLog`, so gameplay can see every clamped spawn.
* `SessionFrame.reports` delivers each contact episode once — the reporter's
  episode dedup; the session adds per-tick draining so frames cannot lose
  an early tick's batch.
* Gravity stays at zero by builder default; F24 decides how gravity is
  applied per the contract's "one owner of gravity" rule.

## Known limitations

* **`spawn_body` direct callers get no preflight.** The swept first-tick
  fix lives in the session's spawn path (the marker is inserted there).
  Any future producer must go through `PhysicsSession::spawn` or insert the
  marker itself — recorded here so a future caller does not silently
  re-open the hole. F23-B's own tests spawn through `spawn_body` directly
  and keep the raw behavior.
* **Rotation on release:** `PhysicsSession` does not yet read or write
  rotational state beyond what `set_body_mode` preserves; authored spawn
  rotation is `Quat::IDENTITY` at the `BodySpec` level. F24's flight model
  owns orientation authoring.
* **Interpolation lag:** presentation trails simulation by up to one fixed
  tick (120 Hz ⇒ ≤ 8.3 ms) — the designed cost of easing rather than
  snapping. Whether the original interpolated at all is unknown.
* **Everything here is designed wiring**: no original tick rate, event
  vocabulary, spawn rule or interpolation behavior was measured. F23-D owns
  the convergence/stability evidence.
