# F52-W3: the modern control profile on the flight input path

Task #782. Follow-up of F52-A limit 6 and F52-D.

## What now exists

* `cs_app::input::assist::apply_assists(base, &GameplayInputs)` builds the
  action map a session flies with. `GameplayInputs` (from
  `Settings::gameplay_inputs()`) is its only reading of the settings.
  * Original rules (no effective assist): mouse motion is unbound and the base
    stick layout is untouched, whatever modern options are stored.
  * `MouseFlight`: mouse motion X yaws, Y pitches. Nothing else changes.
  * `ControllerFlight`: right stick X rolls, right stick Y pitches, left stick
    X yaws. Nothing else changes.
  * `Fov(n)`: not an input mapping; `assist::fov_degrees` hands it to the camera.
* `assist::install` puts that map into an `InputSession` before its first frame.
* `ReplaySubject::with_fidelity(&FidelityLabel)` pins `ChoiceSlot::Assists`
  (`original-rules`, or the comma-joined assist names) with `designed`
  provenance, so a modified and an unmodified run differ in the record alone.

## Limits

1. Every mapping above is **project-designed**. The original PC bindings and
   FOV are unknown (F52-A limit 1, F22); nothing here reproduces them.
2. The base `ActionMap::designed_default()` itself binds mouse motion; the
   original-rules path strips it only through `apply_assists`. A composition
   that skips `install` still has mouse motion bound.
3. **Not wired into the running app.** Nothing in production opens a
   `SettingsSession` yet, and `input::platform` builds its `InputSession` from
   the designed default. Calling `settings_session.gameplay_inputs()` then
   `assist::install` at session construction, `with_fidelity` where the
   replay subject is built, and giving the camera `fov_degrees` need edits in
   `run.rs`/`cli.rs`/camera, outside this task's owner paths. Needs a
   follow-up task.
4. The comparison-metadata writer other than the replay subject (evidence
   reports) was not found under `capture/`; only the replay choice is written.
