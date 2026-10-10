# #1220: `world_geometry` reads `Satisfied` off the residual counters, and M01 plans `11/11` on a branch cut from `main`

Date: 2026-10-10. Task `VS-M01-GEOMETRY-VERDICT-ON-MAIN` (#1220), filed by
`bunny-alpha-2` out of #359 (`VS-M01-RUNTIME`). Feature sheet:
`missions/M01.md` (work order `VS-M01-RUNTIME`, surface `world_geometry`).
Measurement it corrects:
`docs/findings/2026-10-08-m01-lc-world-residual-roles.md` (#771). Capabilities
used: **`retail`** (read-only `$CS_GAME_DIR`, never written) and the
owner-supplied `$CS_ENGINE_IMAGE`. **No original run happened: nothing here is
`verified_original`, and no human played the mission.** Test prefix:
`accept_vs_m01_geometry_verdict_`.

## What changed

`mission_launch::geometry_verdict` asked
`WorldImportReport::partition_records_fog_volume` whether the container left a
record without an answer. #771 measured that accessor to be an **overlap** —
how many grid-named records the original's fog consumer keys — and every
record it covers resolves role `None` once the candidate's own narrow-phase
filter is read. The verdict now reads the report's residual counters instead:

| open term | accessor |
| --- | --- |
| grid-named `fvol*` records still `Unknown` | `objects_unresolved_collision() − objects_unindexed_unresolved()` |
| unindexed geometry-bearing records still `Unknown` | `objects_unindexed_unresolved()` |
| grid records that bind no mesh **and do** store a box | `partition_records() − partition_records_with_mesh() − partition_records_stores_no_geometry()` |
| the axis class | `axis_class() == ObservedTool` |

Each term stays derived from the report's own accessors (no restated strings),
and the pre-#771 reading is documented on the function as the thing it was.

## Measured on the retail installation

```
$ CS_ENGINE_IMAGE="$CS_ENGINE_IMAGE" ./target/debug/cs --cs-path "$CS_GAME_DIR" --mission M01
cs: every launch surface is satisfied but the windowed mission composition is not built
EXIT=1
```

The exit code is `MissionLaunchError::NoRuntime`, not a gap: the plan reaches
the windowed composition this stage does not build. The same read through
`plan_mission_launch` (retail member
`accept_vs_m01_geometry_verdict_retail_m01_s_plan_reports_zero_gaps_and_is_launchable`,
run with `--include-ignored`) reports **eleven of eleven surfaces satisfied,
zero gaps, `launchable() == true`**:

| surface | verdict |
| --- | --- |
| `world_geometry` | satisfied by `world::retail::read_world_container + import_world_container` (144 partition cells, 346 objects, axis `identity` under `f18-world.world-axis-convention-measured`; objects with an `Unknown` collision role: **0**, unindexed geometry-bearing records unresolved: **0**, grid records storing no geometry: **1**) |
| `world_textures` | satisfied by F08 texture decode and F17-B upload adapters |
| `shared_aircraft` | satisfied by `scene_graph_from_gamez` (3317 nodes) |
| `player_configuration` | satisfied by `mission_start::MissionStartConfiguration` |
| `mission_program` | satisfied by `mission_control::ControlLowering` |
| `mission_objectives` | satisfied by `mission_control::ControlLowering` |
| `world_actors` | satisfied by `WorldActorSession launched 3 of 3 decoded world actors` |
| `mission_animations` | satisfied by `MissionAnimationPlayer started 4 of 4 zbd/c1c/m01/mis_anim.zbd rows` (0 refused) |
| `camera_animations` | satisfied by `MissionAnimationPlayer started 3 of 3 zbd/c1c/cam_anim.zbd rows` (0 refused) |
| `mission_audio` | satisfied by `audio::AudibleDevice` over the decoded sound archives |
| `mission_environment` | satisfied by `environment::retail::read_mission_weather` into `EnvironmentSession` |

The retail container exercises **both** corrected terms: four grid-named
`fvol*` records (the overlap, all resolved `None`) and one grid record that
stores no geometry (counted by `partition_records_stores_no_geometry`). Under
the old reading the verdict named both as open — the exact
`1 of 11 launch surfaces are not satisfied` refusal this task was filed to
remove.

## The synthetic member

`crates/cs_app/tests/campaign/vs_m01_geometry_verdict.rs` drives the
production `geometry_verdict` twice through the production
`import_world_container`, over a fixture container whose only records a
container could be silent about are the ones #771 answered:

* a report with `partition_records_fog_volume == 1` beside
  `objects_unresolved_collision == 0` → `Satisfied`;
* a report with one unindexed geometry-bearing record → `Unknown`, naming
  `f18-world.unindexed-collision-role-unmeasured` and never the answered
  overlap or the answered mesh-less half.

The first half is the discriminating one. Reverting the verdict to
`partition_records_fog_volume` and to the un-subtracted mesh-less index makes
it fail with:

```
the overlap and the store's own silence are answers, not gaps: unknown: the container
imports to a WorldDefinition (2 partition cells, 6 objects) with the `identity` axis map
applied as ObservedTool under f18-world.world-axis-convention-measured, and every
unindexed `fvol*` record is classified as fog and resolves role `None` (#716, #771); what
no stage has answered is the collision role of the 1 grid-named `fvol*` volume record
that stores the intersection narrow-phase flag
(f18-world.grid-named-fog-volume-role-unmeasured); and what the 1 grid record that stores
no mesh index drew (...)
```

The fixture helpers are compiled in from
`crates/cs_app/tests/world/import_retail.rs` with the `#[path]` this repo
already uses for shared test modules, because this task's owner paths do not
reach `tests/world/` and duplicating a GameZ container writer would be worse
than running the fixture's own tests a second time.

## Commands run

| command | result |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | exit 0 |
| `cargo test --workspace --locked` | exit 0 |
| `cargo test --workspace --locked -- accept_vs_m01_geometry_verdict_ --include-ignored` | 2 passed, 0 failed |
| `cargo test --test campaign --locked -- accept_vs_m01_runtime_ --include-ignored --nocapture` | 6 passed, 0 failed |
| `CS_ENGINE_IMAGE=… ./target/debug/cs --cs-path "$CS_GAME_DIR" --mission M01` | exit 1, `NoRuntime` (every surface satisfied) |

Nothing under `$CS_GAME_DIR` was written; no derived bytes are committed.

## Claim status

`implemented` only. This is a code-reading correction bound to #771's
measurement plus a retail re-plan; it is not evidence that the original
executable behaved this way, and `human_play` / `human_review` stay with the
owner.
