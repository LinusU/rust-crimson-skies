# M01-RUNTIME-ARCHIVE-REPOINT (#1567): `cs --mission M01` plans `ZBD/C3/M01`

## Result

`launch_mission` declares `("M01", "The Lost Treasure")` and plans it through
`plan_mission_launch_after`, which reads the mission directory out of the
binding's own campaign entry (`program_asset`) and the world group from the
binding's `world_id`. There is no second path table in the runtime: since
M01-ARCHIVE-DECISION (#1565) re-derived the binding, the launch plan for M01
is already:

| Field | Value |
|---|---|
| `catalog_id` | `mission/ch1-m01` |
| `world_id` | `world/c3` |
| `program_id` | `script/c3-m01-zrdr` |
| `mission_dir` | `zbd/c3/m01` |
| `group_dir` | `zbd/c3` |

What #1567 adds is the pin: `accept_vs_m01_runtime_retail_m01_launch_plan_mounts_the_c3_archive`
plans the pair `mission_launch::declared_missions()` hands the launch (so it
cannot drift from `--mission M01`) and asserts the ids above and that the
program surface mounts `zbd/c3/m01/zrdr.zbd`. The remaining `c1c` strings
in `cs_app` tests/doc comments are suites that deliberately measure M06's
archive (`zbd/c1c/m01`) and launch it as M06.

## Baseline surface verdicts for the real M01 (measured, retail, no engine image)

* `world_textures`, `shared_aircraft`, `mission_program`, `mission_objectives`,
  `world_actors` (2 of 2 decoded), `mission_animations` (3 of 4 rows started,
  1 refused), `camera_animations` (3 of 3), `mission_audio`,
  `mission_environment`: **satisfied**.
* `world_geometry`: **unknown** — the container imports (256 partition cells,
  458 objects); what no stage has answered is what the 47 grid records that
  store no mesh index drew.
* `player_configuration`: **unknown** — no source assigns the player's
  airframe for this mission (same state as every campaign mission); in this
  run additionally `CS_ENGINE_IMAGE` was unset.

Because those two surfaces are not satisfied the plan is not launchable, so
`cs --mission M01` refuses with these diagnostics rather than opening a
window; the unknowns are recorded here, not stubbed. The owner smoke
(`--smoke-seconds 45`) therefore cannot run on C3/M01 yet and has no new
baseline; it needs the `world_geometry` 47-record question answered and the
player-airframe source resolved first. The 1 refused animation row in
`zbd/c3/m01/mis_anim.zbd` is likewise unanswered content.
