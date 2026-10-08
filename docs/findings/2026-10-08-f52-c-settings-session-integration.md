# F52-C: the live settings session and its consumers

Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`, `### F52-C`.
Code: `crates/cs_app/src/accessibility/session.rs`.
Tests: `crates/cs_app/tests/accessibility/session.rs` (prefix `accept_f52_c_`, 7 tests).

## What exists

- **`SettingsSession::open(path, safe_defaults)`** is the one place a run
  starts its settings from. It never fails and never writes
  (`store::startup`): no file → designed settings, an unusable file →
  designed settings *with* the [`LoadError`] kept in `recovery()` for the
  screen that reports it, the safe flag → the file is not read at all. The
  file that could not be used is left on disk exactly as it was; only a
  later successful `apply` replaces it.
- **Apply → retry → teardown, with the error propagated.** `apply`
  computes the change from the current settings and validates it *before*
  anything moves, so a refusal (`ApplyError::Invalid`) leaves the session
  and the file untouched. A change that validates but cannot be written is
  live in the session, reported as `ApplyError::Persist`, and stays
  **staged** (`is_staged()`); `retry` and `teardown` flush it again, a
  failed `teardown` leaves the session open so the caller *can* retry, and
  only a successful flush closes it — after which every write is
  `ApplyError::Closed`. Nothing a player set is ever dropped silently.
- **The consumers, from one reading.** `present` is `motion::filter_effects`
  bound to the session's presentation; `page` is `objective_page::from_view`
  bound to it; `gameplay_inputs()` is the only view the simulation may read;
  `control_profile()` returns the labelled `ControlProfile` (`kind`,
  `assists`, `label`) built from a single reading so the assists a consumer
  acts on and the assists a record names cannot disagree; and `project`
  hands a frame's gameplay value, its filtered effects and its fidelity
  metadata to their consumers *together*, which is what makes AC03's
  comparison a property of one projection rather than of two copies of a
  `Settings`.
- **AC03, end to end.** `accept_f52_c_turning_off_shake_and_flash_leaves_gameplay_telemetry_unchanged`
  flies the production `airframe_visual::MissionAircraftSession` (F25's
  exceptional control law, 50 fixed ticks) through `project` under two
  sessions that differ only in `reduce_shake`/`reduce_flash`, in memory and
  on disk: the `TelemetryFrame`, the `GameplayInputs` and the fidelity
  metadata compare equal, while the presented frame drops the shake and the
  flash and keeps `DamageNotice`/`TargetNotice` in order. The same test
  shows each switch governs only its own effect, and that opening a session
  reads the file but never writes it.

## Unknowns and limits (nothing guessed)

1. **No consumer outside this module calls the session yet.** The F52 owner
   paths are `crates/cs_app/src/accessibility/`,
   `crates/cs_content/src/settings.rs`, `crates/cs_app/tests/accessibility/`
   and `docs/findings/`; the places that would open a session at boot, draw
   its presentation and act on its control profile are in other crates'
   modules and are not this task's to edit (F52-D has the same owner paths).
   They are filed as **F52-W1** (#780, `--safe-settings` + boot),
   **F52-W2** (#781, render/audio/effect output) and **F52-W3** (#782,
   flight input + comparison/replay metadata). Until those land, this stage
   proves the seam against real producers — the settings file on disk, the
   real `HudSession` objectives view, the real flight telemetry — but no
   player-visible result exists yet (F52-D needs `gpu`).
2. **Nothing produces `Effect`s.** There is no camera-shake or screen-flash
   request anywhere in the workspace, so `present` and `project` are fed
   authored effect values in the tests. The mapping from gameplay events
   (a hit, an explosion, a target change) to cosmetic effects is designed
   and unwritten — that is F52-W2's work, not something inferred here. What
   is established is the boundary: whatever produces them, the frame the
   player sees goes through `present`, and the required notifications pass.
3. **A failed save leaves the change live in memory.** `apply` keeps a
   staged change rather than rolling the player's toggle back, so the game
   honours it immediately while `is_staged()` and the returned error say it
   is not on disk. Whether a screen should instead roll back and say "not
   saved" is a UI decision for F45/F46's settings screen (F52-W2's context);
   the session reports either way and never pretends.
4. **The flight sample is synthetic.** The telemetry comparison uses the
   F25 synthetic roles, tuning and profile, so it proves that presentation
   settings cannot reach a real control law — not that any original option
   exists or behaves a certain way. The original option set, ranges and
   defaults remain unknown (F52-A finding, limit 1), and the modern
   mouse/controller flight and any explicit FOV stay project-designed
   assists that must be labelled (AC04).
5. **Nothing is drawn, played or flown through the settings yet** beyond the
   data-level consumers above; visual, audible and ordinary-play claims
   remain F52-D's, gated on `gpu`/`audio` capabilities and the owner's
   human review.
