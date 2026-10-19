# PARITY-M01-LAUNCH-CINEMATIC: M01 starts the way the original does (#1642)

Implementer: `Devin SWE-2/swe2-max-1` (SWE-2 Max). Review: pending.

Integration of #1661 (Flight Check), #1663 (the session phase), #1664 (the
letterbox bars), #1665 (the camera playback) and #1668 (the hand-over pose).
Nothing in `run_windowed` needed new wiring: this task pins the ordering the
merged slices already install and proves the sequence end to end.

## The sequence, and where each half lives

1. `launch_mission` resolves `M01` through the declared campaign binding;
   `plan_mission_launch_after` validates the closure; `compose.rs::stage_for`
   reads the stage and `measure_stage_cinematic` derives the cinematic's
   length from the mission's player airframe
   (`measure_launch_cinematic(install_root, row.scene_root)`).
2. `run_windowed` → `build_windowed` → `add_composition`. When no capture
   scenario is set, `FlightCheckPhase::from_retail` opens the real screen and
   `install_flight_check` + `install_flight_check_screen` put it up. The phase
   pauses `Time<Virtual>` (`install_flight_check` also re-asserts the hold in
   `Last` because the playtest's `sync_pause` would undo it), so the fixed
   schedule — physics, the mission host, the objective session, the radio
   queue — runs zero ticks until Fly Mission.
3. Because `flies_directly` is false for the player-facing path and the
   scripted smoke, `stage.launch_cinematic` (Some for `zbd/c3/m01`) installs
   `LaunchCinematic::new(length.ticks(BASELINE_FIXED_HZ))` = 4818 ticks at
   120 Hz (the measured 40.15 s intro, `player_pfighter` — the campaign
   Devastator, #770) plus `read_launch_camera` + `install_launch_camera` for
   the measured `gi_cam*` script.
4. Fly Mission (`Enter` or the `FC_B_FLYMISSION` hotspot) runs the machine's
   `Launch` + `LoadSucceeded` through `Screen::Flight`; `release_clock`
   (PreUpdate) unpauses on the frame after the press. The phase then holds the
   body's `LinearVelocity` aside each `FixedFirst`, gates the flight law and
   weapons on `player_has_control`, hides the cockpit HUD and the actor and
   objective markers, and switches `LaunchLetterbox` on — while
   `play_launch_camera` (PostUpdate, before transform propagation) poses the
   window camera from `elapsed_ticks / 120` seconds of script in the Pandora
   (`piratezep`) frame.
5. Tick 4818 completes the phase: the held velocity is handed back, the bars
   and HUD state restore, and the body stays at the composed start pose —
   the stored record `(-1426, 150, -1813)`, heading 40°, raised 100 m by
   callback 10 (#1668) to `(-1426, 250, -1813)`.

## Tests (prefix `accept_parity_m01_launch_cinematic_`)

* `..._the_check_holds_then_the_cinematic_then_control` (synthetic, CI): over
  the composed synthetic stage the check holds the fixed clock at tick 0 with
  the cinematic armed but unticking; Fly Mission releases it; the release
  frame itself runs zero ticks; six ticks later the phase ends, the bars drop
  and the body's pre-cinematic velocity is handed back untouched.
* `..._retail_stage_binds_the_measured_launch` (`CS_GAME_DIR`): the real
  stage carries `player_pfighter`, 40.15 s → 4818 ticks at 120 Hz, the
  composed start `(-1426, 250, -1813)` at 40°, and `FlightCheckPhase::from_retail`
  opens pending on `Screen::FlightCheck` with `FC_B_FLYMISSION` drawn.
* `..._retail_runs_the_measured_sequence` (`CS_GAME_DIR`): the headless
  production composition drives the whole run — hold, release, 2410 updates
  to hand-over (one release frame + 2409 × 2 ticks), the camera driving all
  four scenes with zero missed Pandora frames — and the body resumes under
  the flight law at the composed pose.

## Owner-command smoke (2026-10-19, release build, `retail,gpu,audio`)

`cs --cs-path "$CS_GAME_DIR" --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY-M01-LAUNCH-CINEMATIC` on the candidate commit: exit
0, 21 checks, 0 failed, 2885 frames, 5676 ticks. Of the sequence:

* `flight_check_screen_drawn`, `flight_check_text_matches`,
  `flight_check_buttons_drawn` and `flight_check_fly_mission` — the check is
  drawn (739‰ of the frame off black) and Fly Mission released through the
  machine.
* `launch_cinematic_plays` — 4818/4818 ticks, handed over, body drift
  0.000 m (ceiling 0.5), turn 0.040° (ceiling 0.5), bars on while playing and
  off after, and the pixel floors hold: ≥990‰ black in the bar rows and ≤20
  HUD-green pixels while held, bars gone and ≥200 HUD-green pixels at
  `cinematic_handover`.
* `launch_cinematic_camera_played` — the measured script drove 2440 frames,
  all 4 scenes seen, 0 frames without the Pandora entity, 7 scene shots.
* `first_control_frame` — tick 4818 (frame 2410) at `(-1426.0, 250.0,
  -1813.0)` m, yaw 40.0°, zero scripted moves.

Screenshots stay in `private/`: `frame_fc00020_flight_check.png` (the full
check), `frame_00005_cinematic_start.png`, `frame_02402_cinematic_end.png`,
`frame_02416_cinematic_handover.png`, `frame_02410_first_control.png` and the
seven `scene_*s` poses among the 28 framebuffers; `report.json` carries every
verdict.

## Still unknown (recorded, not guessed)

* The camera's field of view is unmeasured (no track carries one); the run
  keeps the window's projection (#1665).
* The variant draws come from the original's `rand()` table seeded by the
  clock; the stream is not reproducible from the data, so the run prints the
  seed it drew on the `mission launch camera:` line (#1665).
* Whether the bars slide in/out or appear at once, and exactly when they
  leave, is unmeasured; the implementation switches them at the phase bounds
  (#1664).
* The tick-rate relation is a choice: the original steps a variable
  `game_dt`; the measured length is rounded up to whole 120 Hz ticks (#1663).
* What the original's HUD shows during the cinematic is unmeasured; this
  build hides the cockpit HUD root (`cockpit_hud.rs`) and the actor and
  objective markers (`actor_census.rs`) while the phase plays (#1663, #1704).
* The actor tracks the intro plays (`gi_player1/2`, `gi_pfighter1/2`) are not
  driven yet: the player's plane does not drop past the Pandora and the actors
  keep their composed poses during the cinematic (#1665).
* The hand-over pose's residuals stand in `launch_body::BODY_UNKNOWNS`:
  `node-detach-after-intro`, `cinematic-flag-clear`, `airframe-mode`,
  `network-gate-value`, `velocity-integration`, `no-run` (#1668).

## Files

* `crates/cs_app/tests/campaign/parity_m01_launch_cinematic.rs` (new): the
  task-prefixed suite above.
* `crates/cs_app/tests/campaign/m01_lc_fc_c.rs`: `phase` made `pub(crate)`
  (test-fixture reuse across the suite, visibility only).
* `crates/cs_app/tests/suite_a.rs`: module declaration (wiring).
* `crates/cs_app/tests/evidence_report_parity_m01_launch_cinematic.rs` (new):
  the acceptance-report harness; `docs/findings/evidence/
  PARITY-M01-LAUNCH-CINEMATIC.json` is its validated output.
