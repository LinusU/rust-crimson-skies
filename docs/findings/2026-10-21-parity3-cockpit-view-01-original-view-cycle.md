# PARITY3-COCKPIT-VIEW.01: the original's `Cockpit views` command cycles camera modes 0 → 7 → 6 → 0

Date: 2026-10-21. Task: #1809 (parent #1738). Feature sheet:
`specs/F21-cameras-cockpit-views-and-spyglass.md` (F21-B, F21-D). Predecessors:
`2026-10-11-t542-cockpit-camera-eye-placement.md` (the eye transform behind
modes 6 and 7) and `2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`.

Capability used: `retail` via the owner-supplied decrypted image
`$CS_ENGINE_IMAGE` (SHA-256 `43540fc9…37d75`, the image T542 measured).
**Static analysis only; nothing is `verified_original`** and no original run
happened. Only addresses, offsets and numbers are recorded; no disassembly is
committed. Facts: `data/owner-static/engine-facts.toml` (`[view_cycle]`,
`[[view_cycle_step]]`, claim `OWNER-STATIC-2026-10-21`).

## Measured

1. **Command 49 → handler.** The command-handler registration block pushes
   `0x489340` and `0x31` (49) before the registrar call `0x537bf0`
   (`0x4896ba`). Commands 50–54 register the sibling handlers
   `0x4893b0`/`0x4893d0`/`0x4893f0`/`0x489410`/`0x489430`, each of which sets
   one fixed camera mode (1, 2, 3, 4 and 9) through the same setter.
   Command 49's default keys are `F8` (scancode `0x42`) and joystick button 4
   (`control_binding` row 49 / `cs_types::keymap`).
2. **The handler's mapping.** Unless the player is in the state flagged at
   `[[0x71c298] + 0x91d]` (the handler then does nothing), it reads the camera
   mode at `[[0x64ef78] + 0x14c]` and passes the next mode to the setter
   `0x42c280`:
   0 → 7, 7 → 6, 6 → 0. Any other mode goes to the remembered mode at
   `0x62137c`, which the setter overwrites with the new mode whenever it is 0,
   6 or 7 (`0x42c2ff`) and which is initially 6.
3. **The modes.** The camera dispatcher `0x42c5c0` (T542) reaches the
   cockpit-eye transform `0x42d980` for exactly modes 6 and 7. Mode 0 (table
   entry 0 → `0x42c7f0`, shared with mode 2) is the cycle's third stop.

So one press cycles **mode 0 → mode 7 → mode 6 → mode 0**; the owner's
observation "chase → cockpit → another view" fits mode 0 = chase, 7 and 6 =
two eye-placed views.

## Unknown (rule 4)

* The **names** of modes 0, 6 and 7: no string was found tying them to a name.
  The project calls them `Chase`, `Cockpit` and `SecondEye`
  (`MissionViewMode`) and says so; `Chase` = mode 0 rests on the owner's
  observation plus mode 0 not using the eye transform, not on a measured label.
* What distinguishes mode 6 from mode 7 on screen (canopy, panel, eye offset).
* The mode a mission starts in (the project starts in `Chase`).
* What the `[[0x71c298] + 0x91d]` flag is (the handler is inert while it is
  set); the project does not gate on it.
* Joystick button 4: the input layer has no joystick mission path for the
  original table, so only the keyboard `F8` edge is wired.

## Consumer

`cs_app::mission_session::MissionViewMode` holds the view;
`mint_original_chord_keys` advances it on the `F8` edge (via the control
table, so a rebinding follows), not while paused. Drawing the non-chase views
is the following stages of #1738.
