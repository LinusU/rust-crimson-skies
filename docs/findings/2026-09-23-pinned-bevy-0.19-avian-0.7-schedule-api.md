# Pinned Bevy 0.19 / Avian3d 0.7 schedule API

* Date: 2026-09-23
* Feature/stage: F00-B (`specs/F00-workspace-toolchain-and-first-executable.md`)
* Status: observed in the pinned dependency sources and measured with a probe
  test; original-game schedule semantics remain **unknown** (see below).
* Probe test: `crates/cs_app/tests/accept_f00_b_schedule_ordering.rs`
  (`accept_f00_b_physics_integrates_in_fixed_post_update_before_update`).
* Pin guard: `tools/cs_xtask/tests/accept_f00_b_pinned_dependencies.rs`.

## What is pinned (measured, not assumed)

| Item | Pinned value | Where it is pinned |
| --- | --- | --- |
| `bevy` | `0.19.1` | `Cargo.lock` (requirement `0.19` in `Cargo.toml`) |
| `avian3d` | `0.7.0` | `Cargo.lock` (requirement `0.7` in `Cargo.toml`) |
| toolchain | `1.98.1` | `rust-toolchain.toml` |
| workspace MSRV | `1.98` | `Cargo.toml` `[workspace.package].rust-version` |

Evidence: `cargo tree -p cs_app --depth 1 --locked` reports `avian3d v0.7.0`
and `bevy v0.19.1`; `cs_xtask::pins::Pins::read(...).verify(&INTENDED_BASELINE)`
reads the three files and is asserted by
`accept_f00_b_workspace_pins_the_intended_baseline`. Bumping either patch is a
deliberate edit of `Cargo.lock` and that expectation together; moving to
another minor is rejected by `Pins::verify`.

All line references below are into those exact pinned versions in the local
crate cache (`avian3d-0.7.0`, `bevy_app-0.19.1`, `bevy_time-0.19.1`).

## Observed API

### Avian 0.7.0

* `PhysicsPlugins::default()` is `PhysicsPlugins::new(FixedPostUpdate)`
  (`src/lib.rs:751-755`), and the group adds
  `PhysicsSchedulePlugin::new(self.schedule)` (`src/lib.rs:760`). So with the
  plugin group the scene already builds, **physics runs in Bevy's
  `FixedPostUpdate` schedule**.
* In that schedule Avian configures, chained and
  `.before(TransformSystems::Propagate)`:
  `PhysicsSystems::{First, Prepare, StepSimulation, Writeback, Last}`
  (`src/schedule/mod.rs:77-85`). Our own fixed-phase systems are ordered with
  these sets (`.before(PhysicsSystems::Prepare)` places a system before the
  integration step of the same tick).
* `PhysicsSchedule` (`src/schedule/mod.rs:141`) is run inside
  `PhysicsSystems::StepSimulation` (`src/schedule/mod.rs:106-110`) with a
  `SingleThreadedExecutor` and `ambiguity_detection: LogLevel::Error`
  (`src/schedule/mod.rs:90-94`): ambiguity inside physics is an error, not a
  warning.
* Inside `PhysicsSchedule` the steps are chained
  `PhysicsStepSystems::{First, BroadPhase, NarrowPhase, Solver, Sleeping,
  Finalize, Last}` (`src/schedule/mod.rs:97-104`, enum at `192`).
* `run_physics_schedule` (`src/schedule/mod.rs:235-279`) advances
  `Time<Physics>` by `Time::delta() * relative_speed`, swaps the generic `Time`
  resource to the physics clock while the schedule runs, **skips the step when
  the delta is zero** (`src/schedule/mod.rs:261`) and restores the generic
  clock afterwards. Pausing physics therefore also stops integration.
* Position writeback to `Transform` is
  `PhysicsTransformSystems::PositionToTransform` in `PhysicsSystems::Writeback`
  (`src/physics_transform/mod.rs:116`).
* Naming trap: `PhysicsSet` is a **deprecated alias** of `PhysicsSystems` since
  0.4 (`src/schedule/mod.rs:178-180`), `PhysicsStepSet` likewise
  (`src/schedule/mod.rs:216-218`). Project code must use `PhysicsSystems`;
  the old names still compile but emit deprecation warnings.

### Bevy 0.19 (app and time)

* Per-frame order from `MainScheduleOrder::default`
  (`bevy_app-0.19.1/src/main_schedule.rs:221-236`):
  `First`, `PreUpdate`, `RunFixedMainLoop`, `Update`, `SpawnScene`,
  `PostUpdate`, `Last`.
* `RunFixedMainLoop` runs `FixedMain` "zero to many times, based on how much
  time has elapsed" (`bevy_app-0.19.1/src/main_schedule.rs:34`), and
  `FixedMainScheduleOrder::default` (`bevy_app-0.19.1/src/main_schedule.rs:355-368`)
  is `FixedFirst`, `FixedPreUpdate`, `FixedUpdate`, `FixedPostUpdate`,
  `FixedLast`.
* `bevy_time::fixed::run_fixed_main_schedule` (`bevy_time-0.19.1/src/fixed.rs:243-266`)
  adds `Time<Virtual>::delta()` to the `Time<Fixed>` overstep
  (`fixed.rs:189`) and expends one timestep per loop iteration (`fixed.rs:216`).

**Consequence for this project:** within one frame the effective order is
`PreUpdate → FixedUpdate → FixedPostUpdate (Avian physics) → Update →
PostUpdate`, so gameplay systems placed in `FixedUpdate` run *before* physics
of the same tick, and presentation/`Update` systems see the integrated pose of
that same tick. This matches the phase ordering designed in
`docs/01-ARCHITECTURE.md` (phases 1-4 in the fixed schedules, phase 7 in
`Update`/`PostUpdate`); `docs/01-ARCHITECTURE.md` stays the design source,
this file records what the pinned code actually does.

## Measured with the probe

`SyntheticScene::builder(spec).configure(...)` registers one probe in
`FixedPostUpdate` before `PhysicsSystems::Prepare` and one in `Update`, then
steps the scene and reads the poses back through `SyntheticScene::world()`.
Observed on the pinned pair (values are the y coordinate in meters, start
10.0):

```
frame 1: pre=[]             post=[10.0]
frame 2: pre=[10.0]         post=[10.0, 9.998603]
frame 3: pre=[10.0, 9.998603] post=[10.0, 9.998603, 9.994811]
frame 4: pre=[..., 9.994811] post=[..., 9.994811, 9.988624]
```

* The fixed-phase probe always sees the pose **before** integration, the
  `Update` probe of the same frame sees it **after** — physics integrates in
  `FixedPostUpdate` before `Update`.
* The pose never changes between the `Update` of frame *n* and the fixed-phase
  probe of frame *n+1*: nothing integrates in `Update`/`PostUpdate`.
* `Time<Fixed>::overstep` returns to `0ns` after every frame, so from frame 2
  onwards there is exactly one integration step per `App::update`.

## Observed quirk: the first frame has no fixed step

Frame 1 runs `Update` but **no** `FixedMain`: `Time<Virtual>::delta()` is
`0ns` on the first update. Cause (source, not a guess):
`TimeUpdateStrategy::ManualDuration` calls
`Time<Real>::update_with_duration` → `update_with_instant`
(`bevy_time-0.19.1/src/real.rs:88-91`), and `update_with_instant` returns
early when `last_update` is still `None` (`real.rs:100-105`), i.e. the first
call records the instant without producing a delta. With a zero delta,
`run_fixed_main_schedule` accumulates nothing and `expend()` (`fixed.rs:216`)
fails, so `FixedPostUpdate`/physics do not run in frame 1.

Impact today: `SyntheticScene::step(n)` performs `n` `App::update` calls and
its `Tick` counter reaches `n`, but only `n - 1` physics integrations happen
for the first `n` frames. This is recorded, not fixed here — deciding whether
the scene should pre-seed the fixed-clock accumulator belongs to the fixed-tick
smoke stage (F00-C / AC03, "run a fixed-tick synthetic smoke twice; both end at
the requested tick count"), and a follow-up task was filed with Rally so it is
not silently dropped.

## Unknowns (not guessed)

* The original Crimson Skies tick rate, and which of the phases above the
  original engine ran before/after its integrator: **unknown**, needs the F13+
  compatibility work and original-data evidence.
* Whether F13/F38 require a different schedule variant (e.g. physics in
  `FixedUpdate` instead of `FixedPostUpdate`, or substep configuration other
  than Avian's defaults): **unknown** until those stages measure it; the
  architecture explicitly reserves an evidenced compatibility variant.
* Avian substep count, sleeping and interpolation defaults are taken as
  shipped by the pinned version; no project tuning has been applied yet.

## Sources

* `specs/F00-workspace-toolchain-and-first-executable.md` (intended baseline,
  references [S01]/[S11] in `docs/research/SOURCES.md`).
* Pinned dependency sources: `avian3d-0.7.0`, `bevy_app-0.19.1`,
  `bevy_time-0.19.1` from the local crate cache.
* `docs/01-ARCHITECTURE.md` (project design for the phase ordering).
