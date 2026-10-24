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

## #1581: the launch is plannable; the evidence tests re-pointed, and the owner smoke baseline

Measured on the Apple M5 Max host (Metal), retail + `CS_ENGINE_IMAGE`,
`retail,gpu,audio` listed, on main `aa0cf97b` plus this branch.

### Owner smoke on `ZBD/C3/M01`

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir <dir>`
exits 0: **72 checks, failures: none**, 4082 frames / 7992 ticks, 73 files in the
capture directory. Among them `world_terrain_drawn` 998 per mille off the clear
colour, `world_terrain_textured` 76 per mille (floor 20), `world_terrain_lit`
spread 90, `world_sea_visible` 289 per mille (floor 100; 351 water-tile parts,
none declared by a world object), `hud_objective_current` "1) Find the main
treasure site.", all cockpit and external camera checks, and
`smoke_shots_delivered`.

### Why the capture probe's clear-colour floor could not hold

The capture probe hid the horizon dome, sea and the player and then required
>= 20 per mille of the probe frame to be the window's clear colour, as proof the
hide took effect. C3/M01 never shows the clear colour (a sky gradient with clouds
and an ocean plane are the world's own geometry), so the capture failed with
"holds only 0 of 3686400 pixels of the window's clear colour".

### The replacement (owner decision, 2026-10-18)

* **Structural.** On the probe frame the capture reads `InheritedVisibility` of
  every horizon dome, horizon sea, player aircraft, weather layer and
  precipitation layer and refuses unless each required class (dome, sea, player)
  is present and every entity of every class is hidden. The weather and
  precipitation layers are hidden by the probe too; C3/M01 has none (0 found).
  Trace fields: `dome_found`/`dome_hidden` and the same for `sea`, `player`,
  `weather`, `precipitation`.
* **Pixel.** A reference screenshot (`frame_00028_world_unhidden.png`) is taken
  with nothing hidden four frames before the hide; the probe frame must differ
  from it in at least `MIN_PROBE_DIFFERENCE_PERMILLE` (5) of its pixels, a channel
  difference over the 6-step dither tolerance. Measured on C3/M01: 334561 of
  3686400 pixels, **90 per mille**. `clear_pixels`/`clear_permille` stay in the
  trace as diagnostics (17 per mille here) and no longer gate.

`MIN_CLEAR_COVERAGE_PERMILLE` is gone; the three tests that shared it
(`accept_vs_m01_runtime_evidence_capture_…`, `accept_m01_play_world_windowed_…`,
`evidence_report_m01_play_world_writes_the_acceptance_report`) assert the new
fields instead and keep their other assertions.

### What the structural check found

The first run refused with "the probe frame still draws 1 of 1 player aircraft
entities". The probe's player hide, written in `Update`, was being undone every
frame by `drive_cockpit_camera` (`PostUpdate`), which rewrites the body's
`Visibility` to `Inherited` in the chase view. The old pixel proxy could not see
that the player had never been hidden. The probe now holds the player hidden in
a `PostUpdate` system ordered after `drive_cockpit_camera`.

### The three tests

With `CS_GAME_DIR`, `CS_ENGINE_IMAGE`, `CS_CAPABILITIES=retail,gpu,audio`,
`CS_EVIDENCE_DIR` and `CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}')`:
`accept_vs_m01_runtime_evidence_launch_without_gpu_exits_nonzero_naming_it`,
`accept_vs_m01_runtime_evidence_report_writes_the_acceptance_report` and
`accept_vs_m01_runtime_evidence_capture_writes_png_and_trace_from_the_launch`
pass, as do the two `m01_play_world` retail tests.
