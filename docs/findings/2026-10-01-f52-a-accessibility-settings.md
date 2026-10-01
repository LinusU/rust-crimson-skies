# F52-A: accessibility settings and fidelity boundaries

Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`, `### F52-A`.
Code: `crates/cs_content/src/settings.rs`, `crates/cs_app/src/accessibility/`.
Tests: `crates/cs_app/tests/accessibility/` (prefix `accept_f52_a_`).

## What exists

- `Presentation` (UI scale, subtitles, colour filter, bus levels, reduced
  shake/flash, resolution) and `ModernProfile` (mouse flight, controller
  flight, FOV) are separate types. `Settings::gameplay_inputs()` is built from
  the modern profile only, and only while `ProfileKind::ModernAssist` is active,
  so no presentation setting can reach the simulation (AC03 data half).
- `Settings::fidelity().metadata()` names every active assist (AC04 data half).
- Navigation of the F45-A front end by keyboard only and by controller only
  (AC01), with `Cancel` never firing `Quit`.
- A remap session with cancel, reset and a commit that refuses stranding a device.
- Colour-independent objective cues and UI-scaled metrics (AC02 data half).
- Reduced motion that keeps `DamageNotice`/`TargetNotice` (AC03).
- Atomic settings save and a never-failing safe-defaults startup.

All of it is designed; no original option, range or default was read.

## Unknowns and limits (nothing guessed)

1. **Original options.** Whether the 2000 PC game has any subtitle, scaling,
   shake, flash, mouse-flight or FOV option, and its original FOV, is unknown.
   `ModernAssist::Fov` treats *any* explicit FOV as an assist because the
   original value is unknown. Affects: F52-D, F21 camera. Resolving: F21-B/D,
   F52-D.
2. **Designed ranges.** UI scale 100-300 %, surface 320-16384 px per side and FOV
   40-130 degrees are authored limits, not measurements.
3. **Gamepad UI bindings.** `ActionMap::designed_default` binds no gamepad UI
   action, so a controller cannot navigate with it
   (`accept_f52_a_the_designed_default_map_alone_strands_the_controller`).
   `accessibility::navigation::navigation_map` adds a designed set; moving it
   into the shared default is an F22 change. Affects: F22-C input profiles.
4. **Loadout selection is not focus-driven.** The F45-A machine takes the
   loadout through `select_loadout`, not buttons; the boot-to-launch test
   selects it directly. A device-only dropdown path is F45-B's.
5. **Objective states** (`Pending/Active/Completed/Failed`) and their glyphs are
   designed; the original mission objective states and HUD art are F46-B's/F47's.
   Cue text keys (`objective.*`) are not yet in a locale catalogue (F51-B).
6. **Not wired yet.** The `--safe-settings` startup flag (`cs_app::cli` is not an
   F52-A owner path), persisting the remapped `ActionMap`, applying the
   colour filter/scale/subtitles/bus levels to real render and audio, and
   gameplay assists acting on flight input are F52-B/F52-C. Nothing here
   proves a visible, audible or flown result; F52-D needs `gpu`.
