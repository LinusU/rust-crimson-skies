# Cockpit view 5/5: the owner-command smoke's cockpit phase, and our frame beside the original (Rally #1813)

Task `PARITY3-COCKPIT-VIEW.05`. Capabilities used: `retail`, `gpu`. Nothing here is
`verified_original`; the original frame is the owner-supplied capture linked in #1738
(viewed from `private/`, never committed).

## What the smoke does now

`Phase::Cockpit` (`crates/cs_app/src/mission_session/smoke.rs`) follows the `F1` help phase.
It presses `F8` three times through the production key path (the `ButtonInput<KeyCode>`
the F22 pump reads), 16 frames apart, and at each stop reads the view in `smoke_cockpit`
(after propagation) and requests a framebuffer screenshot:

| stop | label | expected original mode |
|---|---|---|
| 1st press | `cockpit_view` | 7 |
| 2nd press | `cockpit_view_second_eye` | 6 |
| 3rd press | `chase_after_cockpit` | 0 |

The report's `cockpit` object and four checks judge the run: `cockpit_view_cycles`
(modes 7, 6, 0), `cockpit_eye_placement` (camera within 0.05 m of `MissionCockpitEye::pose`
and the body hidden at the pilot eye; at least 1 m from it and the body shown in chase),
`cockpit_dials_drawn` and `cockpit_interior_drawn` (every mesh of the `CockpitDialsRoot` /
`CockpitInteriorRoot` subtree inherits visibility at the pilot eye, none in chase). A part
the dial drive hides itself (warning flags, unlit pips) takes its subtree out of the count;
a hidden root does not. Measured on retail M01: 45/45 interior meshes and every
drive-shown dial mesh drawn at modes 7 and 6, 0 in chase. The thresholds are **designed**
(`MAX_COCKPIT_EYE_ERROR_M`, `MIN_CHASE_FROM_EYE_M`).

Acceptance: `accept_parity3_cockpit_view_05_*` (judge tests; the windowed test runs the
owner's command and asserts the checks and the three PNGs).

## Our frame beside the original

Frames: `private/cockpit-view-05/frame_03048_cockpit_view.png` (2560x1440),
`…_second_eye.png`, `…_chase_after_cockpit.png`; original `private/original-cockpit.png`
(1024x768).

Matches: canopy struts and glass frame, red wing roots, the dash with the carbon pattern,
the compass strip over the dash, the six dials' faces, ROCKETS / attitude ball / GUNS on the
top row, the gunsight glass and base, the blue `[Examine]` caption, the nitro gauge side
panel.

## Unmet / unknown (nothing fixed inside the earlier tasks' paths)

1. **Field of view (unmeasured, the large one).** Ours keeps the chase view's designed 55°
   vertical. In the original the panel is much smaller on screen: the ROCKETS-to-GUNS dial
   spacing (4 stored units) is 0.074 of the frame height there versus 0.124 in ours,
   all six dials fit above the bottom edge, while ours crops the bottom row (MPH, damage
   silhouette, ALT). That ratio would imply a vertical FOV near 80° *if* our eye-to-panel
   distance (stored units read as metres, designed in `.02`) is right; both are unknown,
   so no value was changed. Resolving: recover the cockpit modes' projection from the
   engine image (`.02`'s open item) and confirm the stored-unit scale.
2. **Burl-wood side panels** beside the dash and the **dial text** (`BOOM`, `50 SLUG`,
   `2`, `1980`) were not compared further: the bottom row and glyph cells are off-screen
   in our frame because of (1).
3. Whether the original draws the `rcircle` reticle and the text layer in the cockpit
   (`.04`'s open item) is unchanged.
4. Attitude-ball mapping, flag/arrow thresholds: unchanged from `.04`.
5. Modes 6 and 7 look identical in ours; what distinguishes them is unmeasured (`.01`).
