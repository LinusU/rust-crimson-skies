# VS-M01-RT-WINDOW: the windowed composition, and the launch-gate premise that did not hold

Task: Rally #1215 (`VS-M01-RT-WINDOW`), 2026-10-10.
Implementer: bunny-alpha-1. Branch: `rally/1215-run-m01-s-content-in-the-windowed-compos`.

## What was built

`crates/cs_app/src/mission_session/compose.rs` is the windowed mission
composition and its no-window face:

* `MissionStage` — every record the composition spawns from: the world
  definition, the engine meshes, the mission's load record, the measured
  start pose, the flight law, and the announced load's closure (today: the
  world group's own `gamez.zbd`, its byte length measured from the
  installation).
* `stage_for(install, plan)` — derives the retail stage from
  `MissionContent::prepare` (VS-M01-RT-CONTENT).
* `build_windowed` / `build_headless` — one composition code path, two
  plugin sets, exactly like the playtest's own two faces.
* The F15 LoadingSession handoff: a fresh `ContentSession` over the
  installation's group directory (the mount built the way
  `accept_f15_c_load_session.rs` mounts its fixtures), `announce` before the
  first byte is read, `LoadingSession::run` through `SessionIo` to `Ready`,
  `deliver` → `ReadyBundle::attach` at the ready boundary.
* The world through `load_world`; the player body through `spawn_body` on
  the `Aircraft` layer with the imported mass (`W / FORCE_TO_ACCEL`), the
  mission's `OriginalFlightModel` on the `PlaytestOriginalFlight` record the
  retail playtest's `drive_original_flight` steps each fixed tick, marked
  `PlaytestAircraft` so the playtest's input, chase camera, pause/reset/quit
  and telemetry systems drive it unchanged.
* `teardown(app)`: `unload_world`, despawn of the player body and every
  `LoadedItemBinding` entity, removal of the composition resources and the
  `ExpectedLoad`. A second build in the same process starts from nothing;
  each build mints its own content-session generation (the VFS's own
  counter, never supplied by a caller).
* `launch_mission` now calls `run_windowed` after the `launchable()` gate;
  `MissionLaunchError::NoRuntime` no longer exists on any path. A failure is
  nonzero with the refusing stage's own diagnostics (CLI-EVIDENCE).

The one playtest seam is `playtest::AircraftSpawner` (a resource): with it
absent, `setup_scene`/`perform_reset` behave exactly as before, so
`--playtest` is unchanged; with it present, the mission's player body is
spawned instead and the synthetic scene is not.

## The launch-gate premise: measured twice, corrected mid-task by #1220

The task description stated "**All 11 surfaces measure Satisfied on main
today**". This member's first retail run, against the branch base
`390796d7` (2026-10-10 morning), saw the plan **Blocked**:

```
world_geometry: unknown: ... what no stage has answered is the collision
role of the 4 grid-named `fvol*` volume records
(f18-world.grid-named-fog-volume-role-unmeasured); and what the 1 grid
record that stores no mesh index drew
```

While this task was in flight, Rally #1220
(`VS-M01-GEOMETRY-VERDICT-ON-MAIN`) landed on main (commits `65b304ae`…
`482c8e34`): #771's re-measurement showed `partition_records_fog_volume`
was an **overlap counter** (how many grid-named records the fog consumer
keys), not an open question — every record it covers resolves role `None`
once the candidate's own narrow-phase filter is read. The verdict now reads
the report's residual counters, and the retail plan measures **11/11
satisfied, `launchable() == true`** on current main. The task premise holds
after #1220; the acceptance test `accept_vs_m01_runtime_window_retail_m01_
reaches_the_headless_composition` therefore asserts `launchable()` with the
gap list in its message. Consequences, kept separate:

* **`cs --cs-path <install> --mission M01` now reaches the windowed
  composition on the retail install**: `launch_mission` builds the
  composition through `run_windowed` and blocks until the window closes (or
  fails nonzero with the refusing stage's diagnostics). The reviewer with
  the `gpu` capability runs this; no automated test can open the window,
  which is why the retail member drives the same stage through
  `build_headless`.
* The seam test main had (`accept_vs_m01_runtime_retail_launch_reaches_the_
  composition_seam`, expecting `MissionLaunchError::NoRuntime`) named
  exactly the behavior this task removes; it now asserts the plan-level
  closure (`launchable()`, no `Blocked` gap) and points at the window
  suite's retail member for the composition half.
* A follow-up this member filed mid-task (Rally #1251, "measure the
  collision role of M01 world's grid-named `fvol*` volumes…") is
  **superseded by #1220**, which landed the answer; a note on #1251 says
  so.

## Residues this stage names rather than settles

* **Player collider**: the playtest's declared development box
  (`AIRCRAFT_HALF_EXTENTS_M`), not a measured `pdevastator` extent; the
  airframe visual and its measurement are VS-M01-RT-PLAYER-AIRFRAME-VISUAL
  (#1216). No player mesh is drawn — the windowed HUD label says
  `PLAYER AIRFRAME VISUAL PENDING`.
* **Start-pose heading → orientation**: applied through the production
  `object3d_orientation(0, yaw, 0)` (`M = Ry·Rx·Rz`, #770 §12.2; the yaw
  alone is `Ry(yaw)`). Which body axis the original airframe node's nose
  occupies in the canonical frame was **not** measured here: the spawn
  velocity is placed on the recovered law's own nose (`BODY_FORWARD`, `-Z`)
  of that orientation, and the compass counter-rotation landmark (#436) is
  consistent with it, but the node-nose axis relation stays an open
  measurement. The heading value itself is the original's own conversion
  (`stored_heading_radians`).
* **Start speed**: `RETAIL_START_SPEED_M_S` (the playtest's declared
  development value); the original's player spawn speed is unrecovered
  (#796, unresolved until an original run, #358).
* **Load closure**: this stage announces and delivers the world container
  only. The sound archives, the meshes' derived forms and the mission
  host's per-tick program are the dependent tasks' closure (#1216, #1217).
* **Restart scope**: `R` restarts the player body at the mission's start
  pose with a fresh flight state; a full mission restart (unload, reload,
  new loading attach) is #1217. On exit the composition is torn down.
* **Audio**: no mission-bound audio source is started at this stage, so
  teardown stops none; the archives travel on `MissionContent`.

## Evidence

* `cargo test -p cs_app --test campaign -- accept_vs_m01_runtime_window_
  --include-ignored`: 3 passed (two synthetic members in CI, one retail
  member over `$CS_GAME_DIR` + `$CS_ENGINE_IMAGE`), on both the pre-#1220
  base and the rebased tree containing #1220's verdict change.
* The Blocked retail measurement quoted above was taken against the
  pre-#1220 base and is superseded by #1220's own finding
  (`docs/findings/2026-10-10-vs-m01-geometry-verdict-on-main.md`), which
  the rebased tree's retail member agrees with: `launchable()`, no gaps.
* Full checks on the rebased tree: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings`, `cargo test --workspace --locked`, and the task selection
  above, all green.
